import { SessionStore } from "./store.ts";
// The routing path, without any model. The decision is Fast's: it is told the routes and the user's rules in its instructions,
// and it names the route in the script it writes. Everything after that only checks and carries out: a name is an
// enabled route or it is refused, and the new agent runs on the harness and model of that route. The harnesses here
// are fakes that record what they were asked.
//
//   bun test routes
import { expect, test } from "bun:test";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Agents, type AgentHooks } from "./agents.ts";
import type { Harness, TurnEvent, TurnInput } from "./harness.ts";
import { instructions } from "./fast.ts";
import { checked, enabledRoutes, find, rules, settings, told, type Route } from "./routes.ts";
import { run, type Scope } from "./sandbox.ts";

/** A harness that runs nothing: it notes each turn it was asked for, and the turn ends at once. */
function fake(name: string, models: string[]) {
  const turns: TurnInput[] = [];
  let made = 0;
  const harness: Harness = {
    name,
    can: { fork: true, history: false, steer: false },
    models: async () => models.map((id) => ({ id, about: `${id} of ${name}` })),
    turn: (input) => {
      turns.push(input);
      const session = input.session && !input.fork ? input.session : `${name}-${++made}`;
      async function* events(): AsyncGenerator<TurnEvent> {
        yield { type: "started", session };
        yield { type: "ended", state: "done", text: "Done." };
      }
      return { events: events(), stop: async () => {} };
    },
  };
  return { harness, turns };
}

/** A folder with the routing rules given, or with none. */
const project = (routing?: string) => {
  const folder = mkdtempSync(join(tmpdir(), "live-routes-"));
  if (routing !== undefined) writeFileSync(join(folder, "routing.md"), routing);
  return folder;
};

/** What the `harnesses` setting holds. */
const SETTINGS = { "claude-code": ["fable", "haiku", "gone"], codex: ["astra"], gemini: ["pro"] };

/** The routes of `SETTINGS`, checked against two fake harnesses. */
async function setup() {
  const said: string[] = [];
  const claude = fake("claude-code", ["fable", "sonnet", "haiku"]);
  const codex = fake("codex", ["astra", "sol"]);
  const harnesses = new Map([["claude-code", claude.harness], ["codex", codex.harness]]);
  const folder = project("# Routing\n\n- Small edits: claude-code/haiku.\n");
  const enabled = settings(SETTINGS, (text) => said.push(text))!;
  const routes = await checked(enabledRoutes(enabled, (name) => harnesses.has(name), (text) => said.push(text)), harnesses, (text) => said.push(text));
  return { folder, said, claude, codex, harnesses, routes };
}

const hooks = (): AgentHooks => ({ published: () => {}, holds: () => false, comment: async () => {}, readLog: () => [], owns: () => true, session: () => {}, changed: () => {}, log: () => {} });

/** The last turn a fake was asked for. */
const task = (turns: TurnInput[]) => turns.filter((turn) => turn.message.startsWith("Task ")).at(-1);

const SCOPE: Scope = { messages: [{ id: 1, text: "Rename the heading", page: "/docs/README.md", marked: "" }], updates: [], agents: [], page: null, pages: [] };

test("no settings: nothing is enabled by them, and Fast is told nothing about routes", () => {
  expect(settings(undefined, () => {})).toBeNull();
  const one: Route[] = [{ name: "claude-code", harness: "claude-code", model: null, about: "" }];
  expect(told(one, "some rules")).toBe("");
});

test("the settings give the routes in their order, and the first is the default", async () => {
  const { routes, said } = await setup();
  expect(routes.map((route) => route.name)).toEqual(["claude-code/fable", "claude-code/haiku", "codex/astra"]);
  // A harness nobody knows and a model its harness does not have are dropped, and each is said.
  expect(said.some((text) => text.includes("gemini"))).toBe(true);
  expect(said.some((text) => text.includes("claude-code/gone"))).toBe(true);
  // What each model is for comes from its harness.
  expect(routes[2].about).toBe("astra of codex");
});

test("the check makes no decision: a name is an enabled route, or it is nothing", async () => {
  const { routes } = await setup();
  expect(find(routes, "codex/astra")?.name).toBe("codex/astra");
  expect(find(routes, " Claude-Code/Haiku ")?.name).toBe("claude-code/haiku");
  // A harness alone is not a route: which of its models is Fast's to say, and nothing picks one in its place.
  expect(find(routes, "codex")).toBeUndefined();
  // A model that exists but is not enabled, and a harness that is not there, are no route.
  expect(find(routes, "claude-code/sonnet")).toBeUndefined();
  expect(find(routes, "gemini/pro")).toBeUndefined();
});

test("the decision is in Fast's own instructions: every route, which is the default, and the rules as they are written", async () => {
  const { routes, folder } = await setup();
  const text = told(routes, rules(folder));
  // It is part of the one prompt Fast gets, beside everything else it decides by.
  const prompt = instructions([{ address: "/docs/README.md", title: "Plan" }], text);
  expect(prompt).toContain(text);
  expect(prompt).toContain("spawn(task, route)");
  expect(prompt.indexOf("# Routes")).toBeLessThan(prompt.indexOf("# The pages"));
  expect(instructions([{ address: "/docs/README.md", title: "Plan" }])).not.toContain("# Routes");
  expect(text).toContain("claude-code/fable (the default) — fable of claude-code");
  expect(text).toContain("codex/astra — astra of codex");
  expect(text).toContain("- Small edits: claude-code/haiku.");
  expect(told(routes, null)).toContain("The user has written no rules");
});

test("a script's spawn carries the route it names, and none when it names none", async () => {
  const withRoute = await run("spawn(`The user said: \"${m1}\"`, \"codex/astra\");", SCOPE);
  expect(withRoute.intents).toEqual([{ do: "spawn", prompt: expect.stringContaining("Rename the heading"), route: "codex/astra" }]);
  const without = await run("spawn(`The user said: \"${m1}\"`);", SCOPE);
  expect(without.intents[0]).not.toHaveProperty("route");
  // An empty route is a mistake of the script, which it is told.
  expect(run("spawn(`The user said: \"${m1}\"`, \"\");", SCOPE)).rejects.toThrow("route");
});

test("a new agent runs on the route that was picked: its harness and its model", async () => {
  const { routes, claude, codex, harnesses, folder } = await setup();
  const agents = new Agents(folder, join(folder, "pads"), harnesses, routes, new SessionStore(folder), hooks());
  // What the service does with what a script named: that route when it is one, the default when the script named none,
  // and a refusal, which Fast is told, when the name is no route.
  const dispatch = (name?: string) => {
    const route = name ? find(routes, name) : routes[0];
    if (!route) throw new Error(`${name} is not a route`);
    return route;
  };

  const byRule = agents.spawn({ task: 1, prompt: "small edit" }, dispatch("claude-code/haiku"));
  const byVoice = agents.spawn({ task: 2, prompt: "with codex" }, dispatch("codex/astra"));
  const unnamed = agents.spawn({ task: 3, prompt: "anything" }, dispatch());
  // A route that is not enabled starts nothing: no agent is made on some other route in its place.
  expect(() => dispatch("gemini/pro")).toThrow("not a route");
  expect(() => dispatch("codex")).toThrow("not a route");
  await Bun.sleep(50);

  const on = (id: number) => agents.all().find((agent) => agent.id === id)!;
  expect([on(byRule).harness, on(byRule).model]).toEqual(["claude-code", "haiku"]);
  expect([on(byVoice).harness, on(byVoice).model]).toEqual(["codex", "astra"]);
  expect([on(unnamed).harness, on(unnamed).model]).toEqual(["claude-code", "fable"]);
  expect(agents.all().length).toBe(3);

  // The harness was really asked, with that model.
  expect(task(codex.turns)).toMatchObject({ model: "astra", message: "Task 2\nwith codex" });
  expect(claude.turns.filter((turn) => turn.message.startsWith("Task ")).map((turn) => turn.model)).toEqual(["haiku", "fable"]);
});

test("a follow-up and a fork stay on their agent's route", async () => {
  const { routes, claude, codex, harnesses, folder } = await setup();
  const agents = new Agents(folder, join(folder, "pads"), harnesses, routes, new SessionStore(folder), hooks());
  const first = agents.spawn({ task: 1, prompt: "start" }, find(routes, "codex/astra"));
  await Bun.sleep(50);
  agents.send(first, { task: 2, prompt: "more" });
  const copy = agents.fork(first, { task: 3, prompt: "aside" });
  await Bun.sleep(50);

  const tasks = codex.turns.filter((turn) => turn.message.startsWith("Task "));
  expect(tasks.map((turn) => turn.model)).toEqual(["astra", "astra", "astra"]);
  // The follow-up goes on in the same conversation; the fork in a copy of it.
  expect(tasks[1]).toMatchObject({ session: tasks[0].session === null ? expect.any(String) : tasks[0].session, fork: false });
  expect(tasks[2].fork).toBe(true);
  expect(agents.all().find((agent) => agent.id === copy)).toMatchObject({ harness: "codex", model: "astra" });
  // Nothing of it went to the other harness.
  expect(claude.turns.filter((turn) => turn.message.startsWith("Task "))).toEqual([]);
});

for (const fullFilesystemAccess of [false, true]) {
  test(`new, resumed and forked turns inherit fullFilesystemAccess=${fullFilesystemAccess}`, async () => {
    const { routes, claude, harnesses, folder } = await setup();
    const pads = join(folder, "pads");
    const agents = new Agents(folder, pads, harnesses, routes, new SessionStore(folder), hooks(), 1, fullFilesystemAccess);
    await Bun.sleep(50);
    const first = agents.spawn({ task: 1, prompt: "start" });
    await Bun.sleep(50);
    agents.send(first, { task: 2, prompt: "continue" });
    agents.fork(first, { task: 3, prompt: "branch" });
    await Bun.sleep(50);
    expect(claude.turns).toHaveLength(3);
    for (const turn of claude.turns) {
      expect(turn).toMatchObject({ root: folder, also: [pads], fullFilesystemAccess });
      expect(turn.brief).toContain(fullFilesystemAccess ? "allows reading and writing outside" : "Limit file reads and writes");
    }
  });
}

test("a new agent starts empty, and nothing runs before the first task", async () => {
  const { routes, claude, codex, harnesses, folder } = await setup();
  const agents = new Agents(folder, join(folder, "pads"), harnesses, routes, new SessionStore(folder), hooks());
  await Bun.sleep(50);
  expect(claude.turns).toEqual([]);
  expect(codex.turns).toEqual([]);
  agents.spawn({ task: 1, prompt: "first on codex" }, find(routes, "codex/astra"));
  agents.spawn({ task: 2, prompt: "second on codex" }, find(routes, "codex/astra"));
  await Bun.sleep(50);
  expect(codex.turns.map(({ session, fork, model }) => ({ session, fork, model }))).toEqual([
    { session: null, fork: false, model: "astra" },
    { session: null, fork: false, model: "astra" },
  ]);
});

test("the project's rules replace the ones for every project whole, and without its own those hold", () => {
  const everywhere = project("- Anything: codex/astra.");
  expect(rules(project("- Small edits: claude-code/haiku."), everywhere)).toBe("- Small edits: claude-code/haiku.");
  expect(rules(project(), everywhere)).toBe("- Anything: codex/astra.");
  expect(rules(project(), project())).toBeNull();
});
