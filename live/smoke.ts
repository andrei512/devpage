import { SessionStore } from "./store.ts";
// A check of a harness with the real thing, in a throwaway folder, through the same `Agents` the service uses: a new
// agent gets a small task, a second agent is forked from the first, and the first gets a follow-up. It prints what the agents published and their sessions as the harness gives them back, and fails when they lack
// what they must have. It costs three short turns on the machine's Claude Code login.
//
//   bun smoke.ts          Claude Code
//   bun smoke.ts codex    Codex, on the machine's Codex login
//   bun smoke.ts routes   both at once: one agent on a route of each, on a small model, and the models each harness reports
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Agents, type Published } from "./agents.ts";
import { ClaudeCode } from "./claude.ts";
import { Codex } from "./codex.ts";
import type { Harness } from "./harness.ts";
import { checked, enabledRoutes, find, told } from "./routes.ts";

const folder = mkdtempSync(join(tmpdir(), "live-smoke-"));
const project = join(folder, "project");
mkdirSync(project);
mkdirSync(join(folder, "pads"));
writeFileSync(join(project, "hello.txt"), "hello from the smoke test\n");

const both = process.argv[2] === "routes";
const key = process.argv[2] === "codex" ? "codex" : "claude-code";
const harness: Harness = key === "codex" ? new Codex() : new ClaudeCode();
const harnesses = new Map<string, Harness>(both ? [["claude-code", new ClaudeCode()], ["codex", new Codex()]] : [[key, harness]]);
// For the routes: a small model of each harness, and one that does not exist, which must be dropped.
const complaints: string[] = [];
const routes = both
  ? await checked(enabledRoutes([["claude-code", ["haiku", "no-such-model"]], ["codex", ["gpt-6-luna"]], ["no-such-harness", []]], (name) => harnesses.has(name), (text) => complaints.push(text)), harnesses, (text) => complaints.push(text))
  : [{ name: key, harness: key, model: null, about: "" }];
console.log(`harness: ${both ? "both" : harness.name}; routes: ${routes.map((route) => route.name).join(", ")}`);
const store = new SessionStore(folder);

const running = new Set<number>();
const published: Published[] = [];
let done = () => {};
/** Resolves when no task runs and no agent works. */
const settled = () => new Promise<void>((ok) => (done = ok));
/** Which agent each task was given to. */
const given = new Map<number, number>();
/** How many things were told about sessions as they happened. */
let lines = 0;
const agents: Agents = new Agents(project, join(folder, "pads"), harnesses, routes, store, {
  owns: (agent, task) => given.get(task) === agent,
  session: () => void lines++,
  published: (update) => {
    published.push(update);
    console.log("published", JSON.stringify(update));
    if (update.type === "finished" || update.type === "failed") running.delete(update.task);
  },
  holds: (_agent, task) => running.has(task),
  readLog: () => [],
  changed: () => {
    if (running.size === 0 && agents.all().every((agent) => agent.state !== "working")) done();
  },
  log: (kind, fields) => console.log("log", kind, JSON.stringify(fields).slice(0, 240)),
});

if (both) {
  // One agent on the default route and one on the other harness's, at the same time; then a fork, which stays on its parent's route.
  console.log(told(routes, "- Small things: the default.\n- Code: codex."));
  running.add(1).add(2);
  let wait = settled();
  const one = agents.spawn({ task: 1, prompt: "Say in one sentence which model you are. Then finish." });
  const two = agents.spawn({ task: 2, prompt: "Say in one sentence which model you are. Then finish." }, find(routes, "codex/gpt-6-luna"));
  given.set(1, one).set(2, two);
  await wait;
  running.add(3);
  wait = settled();
  const three = agents.fork(two, { task: 3, prompt: "What did you say a moment ago? Finish with it." });
  given.set(3, three);
  await wait;
  const all = agents.all();
  console.log("\nagents", JSON.stringify(all));
  console.log("complaints", JSON.stringify(complaints));
  const on = (id: number) => all.find((agent) => agent.id === id);
  const end = (task: number) => published.findLast((update) => update.task === task)?.type;
  const problems = [
    routes.map((route) => route.name).join(",") !== "claude-code/haiku,codex/gpt-6-luna" && `the routes are ${routes.map((route) => route.name).join(", ")}`,
    !complaints.some((text) => text.includes("no-such-model")) && "the model that does not exist was not reported",
    !complaints.some((text) => text.includes("no-such-harness")) && "the harness that does not exist was not reported",
    routes.some((route) => !route.about) && "a route does not say what its model is for",
    (on(one)?.harness !== "claude-code" || on(one)?.model !== "haiku") && "the first agent is not on the default route",
    (on(two)?.harness !== "codex" || on(two)?.model !== "gpt-6-luna") && "the second agent is not on the Codex route",
    (on(three)?.harness !== "codex" || on(three)?.model !== "gpt-6-luna") && "the fork is not on its parent's route",
    [1, 2, 3].some((task) => end(task) !== "finished") && `a task did not end as finished: ${[1, 2, 3].map(end).join(", ")}`,
  ].filter(Boolean);
  console.log(problems.length ? `\nFAILED\n- ${problems.join("\n- ")}` : "\nok");
  process.exit(problems.length ? 1 : 0);
}

running.add(1);
let wait = settled();
const first = agents.spawn({ task: 1, prompt: "Read hello.txt, then add a second line to it that says: seen. Then run `wc -l hello.txt` in the shell. Then finish." });
given.set(1, first);
await wait;

// A fork knows what its parent did; a follow-up is a second turn of the same agent.
running.add(2).add(3);
wait = settled();
const second = agents.fork(first, { task: 2, prompt: "Without reading any file: which word did you add to hello.txt a moment ago? Finish with that word in your answer." });
agents.send(first, { task: 3, prompt: "Without reading any file: how many lines did hello.txt have when you counted? Finish with the number." });
given.set(2, second).set(3, first);
await wait;

console.log("\nagents", JSON.stringify(agents.all()));
for (const id of [first, second]) {
  for (const turn of await agents.session(id)) {
    console.log(`\na${id} turn ${turn.n}: ${turn.state}, ${turn.items.length} items`);
    for (const item of turn.items) console.log(`  ${item.state.padEnd(7)} ${item.kind.padEnd(13)} ${item.line.slice(0, 110)}`);
  }
}

const turns = await agents.session(first);
const items = turns[0]?.items ?? [];
const kinds = new Set(items.map((item) => item.kind));
const answer = (task: number) => published.findLast((update) => update.task === task);
const problems = [
  turns.length !== 2 && `expected two turns of the first agent, got ${turns.length}`,
  turns.some((turn) => turn.state !== "done") && `a turn of the first agent ended as ${turns.map((turn) => turn.state).join(", ")}`,
  (await agents.session(second)).length !== 1 && "the fork's session does not begin at its own first turn",
  lines === 0 && "nothing was told about the sessions as they happened",
  harness.can.fork && !/seen/i.test(answer(2)?.text ?? "") && "the fork does not know what its parent did",
  !/\b(2|two)\b/i.test(answer(3)?.text ?? "") && "the follow-up does not know what the first turn did",
  items[0]?.kind !== "user_message" && "the turn does not start with the user's message",
  !items[0]?.line.startsWith("Read hello.txt") && "the user's message is not the prompt without its task line",
  ...(["change", "command", "agent_message"] as const).map((kind) => !kinds.has(kind) && `no ${kind} item`),
  items.some((item) => item.state === "running") && "an item is still running after the turn ended",
  items.some((item) => item.tool?.endsWith("__update")) && "an update was recorded as a tool",
  [1, 2, 3].some((task) => answer(task)?.type !== "finished") && "a task did not end as finished",
  !readFileSync(join(project, "hello.txt"), "utf8").includes("seen") && "the file was not changed",
].filter(Boolean);
console.log(problems.length ? `\nFAILED\n- ${problems.join("\n- ")}` : "\nok");
console.log(`kept in ${folder}`);
process.exit(problems.length ? 1 : 0);
