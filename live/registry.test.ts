// The extendable world's prototype: one path through what the design newly brings.
import { expect, test } from "bun:test";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { deflateSync } from "node:zlib";
import { z } from "zod";
import { BACKEND } from "./fast.ts";
import { provider } from "./provider.ts";
import { Registry, type Caller, type Entry } from "./registry.ts";

const FAST: Extract<Caller, { who: "fast" }> = { who: "fast", run: 1, handover: "h1", used: [] };

/** A part like the computer's: one action for both, which keeps how many ran and adds an entry that wakes Fast only for Fast's own call. */
function computer(registry: Registry) {
  registry.part("mac", ({ z, kind, action }) => {
    kind("ran", { line: ({ script, output }) => `mac  ran ${JSON.stringify(script)}  gave ${JSON.stringify(output)}`, wakes: (_fields, by) => by.who === "fast" });
    action("run", { for: "both", description: "Runs a script on this Mac and gives back what it printed.", input: { script: z.string() } }, ({ script }, running) => {
      if (script === "fail") throw new Error("the script failed");
      running.set("ran", ((running.get("ran") as number) ?? 0) + 1);
      running.add("ran", { script, output: `did ${script}` });
      return `did ${script}`;
    });
  });
}

test("one function, registered once, is a call in Fast's script and a slow agent's tool", async () => {
  const registry = new Registry(":memory:");
  computer(registry);
  const woke: [string, boolean][] = [];
  registry.onEntry = (entry, wakes) => void woke.push([registry.line(entry), wakes]);

  const asked = await registry.run(`mac.run("open Safari")`, FAST);
  expect(asked).toEqual([{ do: "mac.run", input: { script: "open Safari" } }]);

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
  expect(registry.brief()).toBe("mac.run(script)  // Runs a script on this Mac and gives back what it printed.");
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

test("a script that throws asked for nothing, and a wrong input is caught inside the sandbox", async () => {
  const registry = new Registry(":memory:");
  computer(registry);
  await expect(registry.run(`mac.run("one"); mac.run(2)`, FAST)).rejects.toThrow();
  await expect(registry.run(`mac.run()`, FAST)).rejects.toThrow();
  expect(registry.since(0)).toEqual([]);
});

test("who a function is for is held: a slow agent has no call that is for Fast, and a get is never Fast's", async () => {
  const registry = new Registry(":memory:");
  registry.part("voice", ({ z, action, get }) => {
    action("say", { for: "fast", description: "Voice says it.", input: { text: z.string() } }, () => {});
    expect(() => get("heard", { for: "both", description: "What was heard.", input: {} }, () => "")).toThrow("a get is for slow agents only");
  });
  expect(registry.tools(1)).toEqual([]);
  await expect(registry.call("voice.say", { text: "hi" }, { who: "slow", agent: 1 })).rejects.toThrow("voice.say is for fast, not for slow");
});

test("an extension's module in another folder registers a part with what it is handed", async () => {
  const folder = mkdtempSync(join(tmpdir(), "world-extension-"));
  // No import of the program's packages: the schema builder comes with the call.
  writeFileSync(
    join(folder, "world.ts"),
    `export default ({ z, kind, action, widget }) => {
      kind("result", { line: ({ name, passed }) => "test  " + name + "  " + (passed ? "passed" : "failed") });
      action("run", { for: "slow", description: "Runs the tests.", input: { name: z.string() } }, ({ name }, running) => {
        running.set(name, "passed");
        running.add("result", { name, passed: true });
      });
      widget("TestsSection", "panel");
    };`,
  );
  const registry = new Registry(join(folder, "world.sqlite"));
  await registry.load("tests", join(folder, "world.ts"));
  await registry.tools(1)[0].run({ name: "links" });
  expect(registry.state("tests")).toEqual({ links: "passed" });
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
  registry.part("mac", ({ z, kind, action }) => {
    kind("shot", { line: () => "mac  took a screenshot", image: ({ image }) => image, wakes: true });
    action("screenshot", { for: "both", description: "Takes a screenshot.", input: { image: z.string() } }, ({ image }, running) => void running.add("shot", { image }));
  });
  await registry.run(`mac.screenshot(${JSON.stringify(png(255, 0, 0))})`, FAST);
  const wake = registry.wake(registry.since(0));
  expect(wake.content.map((part) => part.type)).toEqual(["input_text", "input_image"]);
});

// The one check that needs the model: run with the key in the environment. It sends one made-up red square.
const PROVIDER = provider(process.env);
test.skipIf(!PROVIDER || !process.env.WORLD_PROTOTYPE_MODEL_CHECK)("Fast's model reads an image an entry carries", async () => {
  const registry = new Registry(":memory:");
  const entry: Entry = { n: 1, at: "", part: "mac", kind: "shot", fields: { image: png(255, 0, 0) } };
  registry.part("mac", ({ kind }) => kind("shot", { line: () => "mac  took a screenshot. Answer with one word: the colour that fills it.", image: ({ image }) => image }));
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
  registry.part("counter", ({ z, action }) => {
    action("add", { for: "page", description: "Adds to the count.", input: { by: z.number() } }, ({ by }, running) => void running.set("count", ((running.get("count") as number) ?? 0) + by));
  });
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

test("failed registration publishes nothing and can be retried", () => {
  const registry = new Registry(":memory:");
  expect(() => registry.part("mac", (p) => {
    p.widget("Panel", "panel");
    p.kind("ran", { line: () => "partial" });
    p.action("run", { for: "both", description: "run", input: {} }, () => {});
    throw new Error("broken");
  })).toThrow("broken");
  expect(registry.parts()).toEqual([]);
  expect(registry.widgets).toEqual([]);
  expect(registry.tools(1)).toEqual([]);
  computer(registry);
});

test("overlapping calls to a part see the previous committed update", async () => {
  const registry = new Registry(":memory:");
  computer(registry);
  await Promise.all(["first", "second"].map(script => registry.call("mac.run", { script }, FAST)));
  expect(registry.state("mac")).toEqual({ ran: 2 });
});

test("Fast checks every registered input and collects mixed calls in script order", async () => {
  const registry = new Registry(":memory:");
  registry.part("counter", ({ z, set }) => set("add", { for: "both", description: "Adds a count", input: { n: z.number().int().positive() } }, ({ n }, ctx) => ctx.set("n", n)));
  const { run } = await import("./sandbox.ts");
  const scope = { messages: [], updates: [], agents: [], page: null, pages: [], handover: [] };
  const result = await run('say("hello"); counter.add(2); say("done")', scope, registry);
  expect(result.intents.map(i => i.do)).toEqual(["say", "world", "say"]);
  await expect(run('say("hello"); counter.add(-1)', scope, registry)).rejects.toThrow();
  await expect(run('counter.add(2); page.go("/missing")', scope, registry)).rejects.toThrow("no page");
  await expect(run('counter.add(2); throw Error("cancel")', scope, registry)).rejects.toThrow("cancel");
  expect(registry.state("counter")).toEqual({});
});

test("a registered built-in keeps its harness spelling and presentation", async () => {
  const registry = new Registry(":memory:");
  registry.part("work", p => p.get("read_log", {
    for: "slow", description: "Read", input: {}, tool: { name: "read_log", always: true },
  }, () => "history"));
  const [tool] = registry.tools(null);
  expect(tool.name).toBe("read_log");
  expect(tool.always).toBe(true);
  expect(await tool.run({})).toEqual({ text: "history" });
});
