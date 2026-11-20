// The registry: what a world model part declares, and what the program does with it.
import { expect, test } from "bun:test";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { deflateSync } from "node:zlib";
import { z } from "zod";
import { run } from "./sandbox.ts";
import { BACKEND, pieces } from "./fast.ts";
import { assemble } from "./prompt.ts";
import { provider } from "./provider.ts";
import { Registry, type Caller, type Entry } from "./registry.ts";

const FAST: Extract<Caller, { who: "fast" }> = { who: "fast", run: 1, handover: "h1", used: [] };
const SCOPE = { agents: [], messages: [], updates: [] };

async function runRegistered(registry: Registry, code: string) {
  const ran = await run(code, SCOPE, registry);
  const calls = ran.intents.map(intent => registry.requested(intent));
  for (const { name, input } of calls) await registry.call(name, input, FAST);
  return calls;
}

/** A part like the computer's: one function for both, which keeps how many ran and adds an entry that wakes Fast only for Fast's own call. */
function computer(registry: Registry) {
  void registry.part("mac", ({ z }) => ({
    state: { ran: z.number().default(0) },
    entries: {
      ran: { line: ({ script, output }) => `mac  ran ${JSON.stringify(script)}  gave ${JSON.stringify(output)}`, wakes: (_fields, by) => by.who === "fast" },
    },
    functions: {
      run: {
        for: "both",
        description: "Runs a script on this Mac and gives back what it printed.",
        input: { script: z.string() },
        run: ({ script }, now) => {
          if (script === "fail") throw new Error("the script failed");
          now.state.ran += 1;
          now.add("ran", { script, output: `did ${script}` });
          return `did ${script}`;
        },
      },
    },
  }));
}

test("one function, declared once, is a call in Fast's script and a slow agent's tool", async () => {
  const registry = new Registry(":memory:");
  computer(registry);
  const woke: [string, boolean][] = [];
  registry.onEntry = (entry, wakes) => void woke.push([registry.line(entry), wakes]);

  const asked = await runRegistered(registry, `mac.run("open Safari")`);
  expect(asked).toEqual([{ name: "mac.run", input: { script: "open Safari" } }]);

  const [tool] = registry.tools(2);
  expect(tool.name).toBe("mac_run");
  expect(z.toJSONSchema(z.object(tool.input)).required).toEqual(["script"]);
  expect(await tool.run({ script: "front app" })).toEqual({ text: "did front app" });

  // Fast's call returned nothing to the script, so its entry wakes Fast; the agent got its answer, so its entry does not.
  expect(woke).toEqual([
    [`mac  ran "open Safari"  gave "did open Safari"`, true],
    [`mac  ran "front app"  gave "did front app"`, false],
  ]);
  expect(registry.state("mac")).toEqual({ ran: 2 });
  expect(registry.brief()).toBe(`${"mac.run(script)".padEnd(27)} // Runs a script on this Mac and gives back what it printed.`);
});

test("state and entries are written together or not at all", async () => {
  const registry = new Registry(":memory:");
  computer(registry);
  const sent: Record<string, unknown>[] = [];
  registry.onState = (_part, state) => void sent.push(state);
  await registry.call("mac.run", { script: "one" }, FAST);
  await expect(registry.call("mac.run", { script: "fail" }, FAST)).rejects.toThrow("the script failed");
  expect(registry.state("mac")).toEqual({ ran: 1 });
  expect(registry.since(0).map((entry) => entry.fields.script)).toEqual(["one"]);
  // A page is sent the part's whole state, once per change.
  expect(sent).toEqual([{ ran: 1 }]);
});

test("a part's state is what it declared: a new session has each field's default, and no other field or value is saved", () => {
  const registry = new Registry(":memory:");
  void registry.part("counter", ({ z }) => ({ state: { count: z.number().default(0), label: z.string().optional() } }));
  expect(registry.state("counter")).toEqual({ count: 0 });
  expect(() => registry.step("counter", ({ state }) => void (state.other = 1))).toThrow("counter has no state field other");
  expect(() => registry.step("counter", ({ state }) => void (state.count = "many"))).toThrow();
  registry.step("counter", ({ state }) => void (state.count = 3));
  expect(registry.state("counter")).toEqual({ count: 3 });
});

test("a script that throws asked for nothing, and a wrong input is caught inside the sandbox", async () => {
  const registry = new Registry(":memory:");
  computer(registry);
  await expect(runRegistered(registry, `mac.run("one"); mac.run(2)`)).rejects.toThrow();
  await expect(runRegistered(registry, `mac.run()`)).rejects.toThrow();
  expect(registry.since(0)).toEqual([]);
});

test("who a function is for is held: a slow agent has no call that is for Fast", async () => {
  const registry = new Registry(":memory:");
  void registry.part("voice", ({ z }) => ({
    functions: { say: { for: "fast", description: "Voice says it.", input: { text: z.string() }, run: () => {} } },
  }));
  expect(registry.tools(1)).toEqual([]);
  await expect(registry.call("voice.say", { text: "hi" }, { who: "slow", agent: 1 })).rejects.toThrow("voice.say is for fast, not for slow");
});

test("an extension's module in another folder declares a part with what it is handed", async () => {
  const folder = mkdtempSync(join(tmpdir(), "world-extension-"));
  // No import of the program's packages: the schema builder comes with the call.
  writeFileSync(
    join(folder, "part.ts"),
    `export default ({ z }) => ({
      state: { results: z.record(z.string(), z.string()).default({}) },
      entries: { result: { line: ({ name, passed }) => "test  " + name + "  " + (passed ? "passed" : "failed") } },
      functions: {
        run: {
          for: "slow",
          description: "Runs the tests.",
          input: { name: z.string() },
          run: ({ name }, now) => {
            now.state.results = { ...now.state.results, [name]: "passed" };
            now.add("result", { name, passed: true });
          },
        },
      },
      widgets: { TestsSection: "panel" },
    });`,
  );
  const registry = new Registry(join(folder, "world.sqlite"));
  await registry.load("tests", join(folder, "part.ts"));
  await registry.tools(1)[0].run({ name: "links" });
  expect(registry.state("tests")).toEqual({ results: { links: "passed" } });
  expect(registry.widgets).toEqual([{ part: "tests", name: "TestsSection", place: "panel" }]);
  // The store is a file: another Registry on it finds what was written.
  expect(new Registry(join(folder, "world.sqlite")).since(0).map((entry) => entry.kind)).toEqual(["result"]);
});

/** A 64 by 64 PNG of one colour, made here: no file and nothing of the user's. */
function png(red: number, green: number, blue: number) {
  const size = 64;
  const raw = new Uint8Array((size * 3 + 1) * size);
  for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) raw.set([red, green, blue], y * (size * 3 + 1) + 1 + x * 3);
  const crc = (bytes: Uint8Array) => {
    let c = ~0;
    for (const byte of bytes) {
      c ^= byte;
      for (let k = 0; k < 8; k++) c = c & 1 ? (c >>> 1) ^ 0xedb88320 : c >>> 1;
    }
    return ~c >>> 0;
  };
  const chunk = (type: string, data: Uint8Array) => {
    const body = new Uint8Array([...new TextEncoder().encode(type), ...data]);
    const out = new Uint8Array(12 + data.length);
    const view = new DataView(out.buffer);
    view.setUint32(0, data.length);
    out.set(body, 4);
    view.setUint32(8 + data.length, crc(body));
    return out;
  };
  const header = new Uint8Array(13);
  new DataView(header.buffer).setUint32(0, size);
  new DataView(header.buffer).setUint32(4, size);
  header.set([8, 2, 0, 0, 0], 8);
  const bytes = new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10, ...chunk("IHDR", header), ...chunk("IDAT", deflateSync(raw)), ...chunk("IEND", new Uint8Array())]);
  return `data:image/png;base64,${Buffer.from(bytes).toString("base64")}`;
}

test("an entry's image goes into what a wake-up sends", async () => {
  const registry = new Registry(":memory:");
  void registry.part("mac", ({ z }) => ({
    entries: { shot: { line: () => "mac  took a screenshot", image: ({ image }) => image, wakes: true } },
    functions: { screenshot: { for: "both", description: "Takes a screenshot.", input: { image: z.string() }, run: ({ image }, now) => void now.add("shot", { image }) } },
  }));
  await runRegistered(registry, `mac.screenshot(${JSON.stringify(png(255, 0, 0))})`);
  const wake = registry.wake(registry.since(0));
  expect(wake.content.map((part) => part.type)).toEqual(["input_text", "input_image"]);
});

// The one check that needs the model: run with the key in the environment. It sends one made-up red square.
const PROVIDER = provider(process.env);
test.skipIf(!PROVIDER || !process.env.WORLD_PROTOTYPE_MODEL_CHECK)("Fast's model reads an image an entry carries", async () => {
  const registry = new Registry(":memory:");
  const entry: Entry = { n: 1, at: "", part: "mac", kind: "shot", fields: { image: png(255, 0, 0) } };
  void registry.part("mac", { entries: { shot: { line: () => "mac  took a screenshot. Answer with one word: the colour that fills it.", image: ({ image }) => image } } });
  const response = await fetch(PROVIDER!.responses, {
    method: "POST",
    headers: { ...PROVIDER!.headers, "content-type": "application/json" },
    body: JSON.stringify({ model: process.env.WORLD_PROTOTYPE_MODEL || BACKEND, input: [registry.wake([entry])], store: false, reasoning: { effort: "low" } }),
    signal: AbortSignal.timeout(45_000),
  });
  const answer: any = await response.json();
  if (!response.ok) throw new Error(`${response.status}: ${JSON.stringify(answer).slice(0, 400)}`);
  const text = answer.output.flatMap((item: any) => (item.type === "message" ? item.content : [])).map((part: any) => part.text).join("");
  expect(text.toLowerCase()).toContain("red");
}, 60_000);

test("a function for the page is a widget's to call, and nobody else's", async () => {
  const registry = new Registry(":memory:");
  void registry.part("counter", ({ z }) => ({
    state: { count: z.number().default(0) },
    functions: { add: { for: "page", description: "Adds to the count.", input: { by: z.number() }, run: ({ by }, now) => void (now.state.count += by) } },
  }));
  const sent: [string, Record<string, unknown>][] = [];
  registry.onState = (part, state) => void sent.push([part, state]);
  await registry.call("counter.add", { by: 2 }, { who: "page" });
  expect(sent).toEqual([["counter", { count: 2 }]]);
  expect(registry.parts()).toEqual(["counter"]);
  // It is no tool of a slow agent's, and no call in Fast's script.
  expect(registry.tools(1)).toEqual([]);
  expect(registry.brief()).toBe("");
  await expect(registry.call("counter.add", { by: 1 }, FAST)).rejects.toThrow("counter.add is for page, not for fast");
});

test("a part that cannot be registered leaves nothing of itself, and can be registered again", () => {
  const registry = new Registry(":memory:");
  const log = (tool: string) => ({ for: "slow" as const, description: "Read", tool: { name: tool }, run: () => "" });
  void registry.part("work", { functions: { read: log("read_log") } });
  const second = (tool: string) => ({ entries: { ran: { line: () => "ran" } }, functions: { read: log(tool) }, widgets: { Panel: "panel" as const } });
  expect(() => registry.part("mac", second("read_log"))).toThrow("The slow-agent tool read_log is registered twice");
  expect(() => registry.part("broken", () => { throw new Error("broken"); })).toThrow("broken");
  expect(registry.parts()).toEqual(["work"]);
  expect(registry.widgets).toEqual([]);
  expect(registry.tools(1).map((tool) => tool.name)).toEqual(["read_log"]);
  void registry.part("mac", second("mac_log"));
  expect(registry.parts()).toEqual(["work", "mac"]);
});

test("overlapping calls to a part see the previous committed update", async () => {
  const registry = new Registry(":memory:");
  computer(registry);
  await Promise.all(["first", "second"].map(script => registry.call("mac.run", { script }, FAST)));
  expect(registry.state("mac")).toEqual({ ran: 2 });
});

test("Fast's script has every call checked as it is made: its input, then the function's own check, and nothing is done when one fails", async () => {
  const registry = new Registry(":memory:");
  void registry.part("session", ({ z }) => ({ functions: { say: { for: "fast", syntax: "say(text)", description: "Speak", input: { text: z.string() }, run: () => {} } } }));
  void registry.part("counter", ({ z }) => ({
    state: { n: z.number().default(0) },
    functions: {
      add: {
        for: "both",
        description: "Adds a count",
        input: { n: z.number().int().positive() },
        // The check has what the part last showed Fast and the script's earlier calls, and what it returns is the call's answer in the script.
        check: ({ n }, shown, earlier) => {
          const most = Number(shown["The most"]);
          if (n > most) throw new Error(`the most is ${most}`);
          return earlier.length;
        },
        run: ({ n }, now) => void (now.state.n = n),
      },
    },
    showFast: () => ({ "The most": "5" }),
  }));
  const shown = await registry.shown();
  expect(shown).toEqual({ counter: { "The most": "5" } });
  const result = await run('say("hello"); const before = counter.add(2); say(`done after ${before}`)', SCOPE, registry, shown);
  expect(result.intents).toEqual([{ do: "say", text: "hello" }, { do: "world", name: "counter.add", input: { n: 2 } }, { do: "say", text: "done after 0" }]);
  await expect(run('say("hello"); counter.add(-1)', SCOPE, registry, shown)).rejects.toThrow();
  await expect(run("counter.add(2); counter.add(6)", SCOPE, registry, shown)).rejects.toThrow("the most is 5");
  await expect(run('counter.add(2); throw Error("cancel")', SCOPE, registry, shown)).rejects.toThrow("cancel");
  expect(registry.state("counter")).toEqual({ n: 0 });
});

test("a function of the program's own keeps its tool's name and how the harness is handed it", async () => {
  const registry = new Registry(":memory:");
  void registry.part("work", { functions: { read_log: { for: "slow", description: "Read", tool: { name: "read_log", always: true }, run: () => "history" } } });
  const [tool] = registry.tools(null);
  expect(tool.name).toBe("read_log");
  expect(tool.always).toBe(true);
  expect(await tool.run({})).toEqual({ text: "history" });
});

test("a call the sandbox itself provides is checked against its function's input before the script ends", async () => {
  const registry = new Registry(":memory:");
  void registry.part("session", ({ z }) => ({
    functions: { say: { for: "fast", syntax: "say(text)", input: { text: z.literal("allowed") }, description: "Speak", run: () => {} } },
  }));
  await expect(run('say("allowed"); say("wrong")', SCOPE, registry)).rejects.toThrow();
});

test("Fast can run the call as its list has it for every legal shape of package id", async () => {
  const registry = new Registry(":memory:");
  for (const id of ["unit-tests", "42", "class", "a2", "Object", "counter"]) {
    void registry.part(id, { functions: { run: { for: "fast", description: "Run", run: () => {} } } });
  }
  for (const [i, line] of registry.brief().split("\n").entries()) {
    expect(await runRegistered(registry, line)).toEqual([{ name: registry.parts()[i] + ".run", input: {} }]);
  }
});

test("an entry that cannot be read survives and the next wake-up can still be delivered", async () => {
  const registry = new Registry(":memory:");
  void registry.part("tests", ({ z }) => ({
    state: { done: z.boolean().default(false) },
    entries: {
      result: {
        line: f => f.result.name.toUpperCase(),
        image: f => f.result.picture.url,
        wakes: f => f.result.complete,
      },
    },
    functions: {
      run: {
        for: "fast",
        description: "Run",
        run: (_, now) => {
          now.state.done = true;
          now.add("result", { legacy: true });
          now.add("result", { result: { name: "current", picture: { url: "image.png" }, complete: true } });
        },
      },
    },
  }));
  const notices: boolean[] = [];
  registry.onEntry = (entry, wakes) => { registry.line(entry); notices.push(wakes); };
  await registry.call("tests.run", {}, FAST);
  expect(notices).toEqual([false, true]);
  expect(registry.state("tests")).toEqual({ done: true });
  expect(registry.wake(registry.since(0)).content).toEqual([
    { type: "input_text", text: "tests.result\nCURRENT" }, { type: "input_image", image_url: "image.png" },
  ]);
});

test("work that outlasts a call saves each result as its own step, and a call that failed starts none", async () => {
  const registry = new Registry(":memory:");
  const states: unknown[] = [];
  const complete = Promise.withResolvers<void>();
  registry.onState = (_part, state) => void states.push(state.count);
  void registry.part("stream", ({ z }) => ({
    state: { count: z.number().nullable().default(null) },
    entries: { done: { line: () => "done" } },
    functions: {
      start: {
        for: "slow",
        input: { fail: z.boolean() },
        description: "Start results",
        run: ({ fail }, now) => {
          now.state.count = 0;
          now.follow(async publish => {
            await publish(later => void (later.state.count = 1));
            await publish(later => { later.state.count += 1; later.add("done", {}); });
            complete.resolve();
          });
          if (fail) throw new Error("Start failed");
        },
      },
    },
  }));
  await expect(registry.call("stream.start", { fail: true }, { who: "slow", agent: 1 })).rejects.toThrow("Start failed");
  expect(states).toEqual([]);
  await registry.call("stream.start", { fail: false }, { who: "slow", agent: 1 });
  await complete.promise;
  expect(states).toEqual([0, 1, 2]);
  expect(registry.since(0).map(x => x.kind)).toEqual(["done"]);
});

test("a part adds its lines to a task as it is given", () => {
  const registry = new Registry(":memory:");
  void registry.part("counter", ({ z }) => ({
    state: { count: z.number().default(4) },
    addToTask: (state, task) => (task.page ? [`The count is ${state.count}.`] : []),
  }));
  expect(registry.added({ prompt: "Do it", page: "/README.md", marked: "" })).toEqual(["The count is 4."]);
  expect(registry.added({ prompt: "Do it", page: null, marked: "" })).toEqual([]);
});

test("a part's text for an agent is placed in that agent's prompt: under a heading the template has, or as a section of its own", async () => {
  const folder = mkdtempSync(join(tmpdir(), "part-text-"));
  writeFileSync(join(folder, "world.js"), "export default () => ({ functions: {} });");
  writeFileSync(join(folder, "fast.md"), "# The notes\n\nA note is kept until the user drops it.\n\n# How you talk\n\n- A note is read out as it is.\n");
  const registry = new Registry(":memory:");
  await registry.load("notes", join(folder, "world.js"));
  expect(registry.texts()).toEqual([{ part: "notes", folder }]);
  const all = pieces(registry);
  expect(all.find((piece) => piece.from === "notes: fast.md · The notes")?.text).toContain("A note is kept until the user drops it.");
  const talk = all.findIndex((piece) => piece.from === "core: fast.md · How you talk");
  expect(all[talk + 1]).toEqual({ from: "notes: fast.md · How you talk", text: "\n\n- A note is read out as it is." });
});

test("a part's line under a GPT-Live label of Voice's template goes at the end of that label's list", () => {
  const folder = mkdtempSync(join(tmpdir(), "part-voice-"));
  writeFileSync(join(folder, "voice.md"), "# Backend tools\n\n- Notes: keep and read the user's notes.\n");
  const text = assemble("voice", [{ part: "notes", folder }]).map((piece) => piece.text).join("");
  const tools = text.slice(text.indexOf("Backend tools:"), text.indexOf("Generate <|SpawnThinking|> when:"));
  expect(tools.trimEnd().endsWith("- Notes: keep and read the user's notes.")).toBe(true);
  expect(text).toContain("- Notes: keep and read the user's notes.\n\nGenerate <|SpawnThinking|> when:");
});
