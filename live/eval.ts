import { Registry, type FastFunction } from "./registry.ts";
import { z } from "zod";
import { SessionStore } from "./store.ts";
// The evals of Fast. A case is one moment of a session: what Fast had been shown until then, the wake-up, and what a
// right script does there. The moment is put to the model through the same code the service runs (the brief, the
// request, the sandbox, the second try), several times, and each script is checked: its calls in code, and the words
// of a spoken line or of a prompt for an agent by a judge model.
//
//   bun eval.ts                           the setup the service runs
//   bun eval.ts gpt-6-luna:medium         another one, as model[:effort[:tier]]; several are compared
//   bun eval.ts sets                      the sets of cases, by behavior, with how many cases each has
//   bun eval.ts --set routing,scratch-pad the cases of those sets; `--set held-out` for the held-out ones
//   bun eval.ts --failed                  the cases that did not pass every time in the last run that was saved
//   bun eval.ts --set routing --list      which cases a run would be, and how many calls: nothing is run
//   bun eval.ts --only follow --runs 5    the cases with "follow" in their name, five times each
//
// Without `--set`, `--failed` or `--only`, a run is every set but `held-out`: those cases were never used to change the
// brief, and stay a fair check only while they are run rarely, to check a change at its end.
//   bun eval.ts show <case>               what Fast is shown in a case
//   bun eval.ts freeze <session>          a recorded voice session, from the log, as a file cases can use
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { BACKEND, EFFORT, instructions, moved, request, roster, script, wakeText, type Seen } from "./fast.ts";
import { query } from "@anthropic-ai/claude-agent-sdk";
import { price } from "./cost.ts";
import { NEEDS, provider } from "./provider.ts";
import { HOME, stateOf } from "./home.ts";
import { map } from "./pages.ts";
import { run as execute, type Intent, type Scope } from "./sandbox.ts";
import type { Entry, Message, TaskGiven, Update } from "./world.ts";
import { CASES, SETS } from "./evals/cases.ts";

/** The project a session is frozen from: the folder `freeze` is run in. */
const ROOT = process.cwd();
/** Where every run is saved. */
const RUNS = join(HOME, "live", "evals");
const SESSIONS = join(import.meta.dir, "evals/sessions");
/** The model that judges words. It runs on the Claude subscription, as the slow agents do: judging is not billed. */
const JUDGE = "sonnet";
/** How many calls of a model run at once. */
const AT_ONCE = 4;

// ---------------------------------------------------------------------------------------------------------------------
// What a case is

type Kind = Intent["do"];

/** One call a right script makes. Every field given must be the call's. */
export interface Want {
  do: Kind | Kind[];
  agent?: number;
  /** The route a `spawn` must name, or one of several; "" is naming none, which is the default route. */
  route?: string | string[];
  address?: string;
  /** An address the call must not have: a new scratch pad, not this one. */
  not?: string;
  where?: "up" | "down" | "top" | "bottom";
  open?: boolean;
  /** Words the call's text must have: a prompt's, a line's, or the quote of a part of the page. */
  has?: string[];
  /** The most words the call's text may have: a line that only says the work is taken on. Counted, not judged. */
  words?: number;
}

export interface Expect {
  /** The calls the script must make: each is matched by a call of its own. */
  does: Want[];
  /** The kinds of call it may make besides. A case that names none allows none. */
  may?: Kind[];
  /** What must hold for the words the user hears. Judged. */
  line?: string[];
  /** What must hold for the prompt an agent gets. Judged. */
  prompt?: string[];
}

/** One thing that happens in a written moment. Every step but the last has the script Fast answered it with. */
export interface Step {
  /** What the user says, as the transcript has it. */
  he?: string;
  /** The text the user has marked when the user says it. */
  marked?: string;
  /** The page the user has opened by themselves before it: one whose text the session has. */
  opened?: string;
  /** The user closed the canvas themselves before it, with its cross. */
  closed?: true;
  /** Updates the agents published before it. The task is the agent's newest when none is given. */
  updates?: { agent: number; task?: number; type: Update["type"]; text: string }[];
  script?: string;
}

export interface Case {
  name: string;
  /** The sets it is in, by behavior: what a run can be limited to. `held-out` keeps it out of an everyday run. */
  sets?: string[];
  /** What it is there to catch. */
  why: string;
  /** The session it happens in: a file in evals/sessions, recorded or written. */
  session: string;
  /** A moment of the session itself: this wake-up of it. */
  run?: number;
  /** A written moment: the steps happen after this wake-up of the session, and the last one is the moment. */
  after?: number;
  steps?: Step[];
  /** What Fast is told about the routes and the user's rules, as `told` of routes.ts writes it. None: there is one route, and nothing to pick. */
  routes?: string;
  /** The scratch pads there are before the first step, each with its idea's title and its text as the user sees it. A pad a script makes is added, with its title alone. */
  pads?: Record<string, { title: string; text: string }>;
  expect: Expect;
}

// ---------------------------------------------------------------------------------------------------------------------
// A session: recorded from the log with `freeze`, or written by hand in the same shape

interface Run {
  run: number;
  /** The numbers of the world's entries it was shown. */
  entries: number[];
  /** The ids of the user's messages that woke it. */
  said: number[];
  page: { address: string; marked: string; panel: boolean } | null;
  /** Whether it was shown the page's text. */
  unseen: boolean;
  /** What Fast was told, as it was. */
  wake: string;
  images?: string[];
  /** What it answered: one script, or two when the first threw. */
  scripts: { code: string; output: string }[];
}

interface Recorded {
  name: string;
  pages: { address: string; title: string }[];
  /** The page texts Fast was shown, by address. */
  texts: Record<string, string>;
  /** The first message of Fast's context, and the ids in it. */
  start: string;
  functions?: FastFunction[];
  held: { messages: number[]; updates: number[]; agents: number[] };
  /** The world, until the session's end. */
  entries: Entry[];
  runs: Run[];
}

function recorded(name: string): Recorded {
  const file = join(SESSIONS, `${name}.json`);
  if (!existsSync(file)) throw new Error(`No recorded session ${name} in ${SESSIONS}.`);
  return JSON.parse(readFileSync(file, "utf8"));
}

/** Makes a session file from the log and the world of the project it is run in: `bun eval.ts freeze <start of the session's id>`. */
export function freeze(session: string, root = ROOT, destination = SESSIONS) {
  const store = new SessionStore(stateOf(root), "read");
  const log: any[] = store.logs();
  const mine = log.filter((entry) => typeof entry.session === "string" && entry.session.startsWith(session));
  const wakes = mine.filter((entry) => entry.who === "fast" && entry.kind === "wake");
  if (wakes.length === 0) throw new Error(`The log has no wake-up of a session ${session}.`);
  const context = log.filter(entry => entry.kind === "context_built" && entry.ts <= wakes[0].ts).at(-1);
  const start: string = context?.text ?? "";
  const world = store.world();
  const ended = mine.at(-1).ts;
  const texts: Record<string, string> = {};
  // What a wake-up's text says of the page is read back from it: the log has the text Fast was told and no more.
  const runs: Run[] = wakes.map((wake) => {
    const text: string = wake.text;
    const page = /\n<<<\n([\s\S]*?)\n>>>(?:\n|$)/.exec(text);
    const images = [...store.events.images(wake.entries).values()];
    if (page && wake.page) texts[wake.page] = page[1];
    const marked = /\. Marked now: (".*")\. The side panel is (?:open|closed)\./.exec(text);
    return {
      run: wake.run,
      entries: wake.entries,
      said: [...(/The user just said ((?:m\d+(?:, )?)+)\./.exec(text.split("\n")[0])?.[1] ?? "").matchAll(/m(\d+)/g)].map((found) => Number(found[1])),
      page: wake.page ? { address: wake.page, marked: marked ? JSON.parse(marked[1]) : "", panel: /The side panel is open\./.test(text) } : null,
      unseen: Boolean(page),
      wake: text,
      ...(images.length ? { images } : {}),
      scripts: mine
        .filter((entry) => entry.who === "fast" && entry.run === wake.run && (entry.kind === "script" || entry.kind === "script_failed"))
        .map((entry) => ({ code: entry.text, output: entry.kind === "script" ? "ok" : `${entry.error}\nNothing was done.${entry.attempt === 1 ? " Write the script again." : ""}` })),
    };
  });
  const ids = (letter: string) => [...start.matchAll(new RegExp(`^\\s*${letter}(\\d+)  `, "gm"))].map((found) => Number(found[1]));
  const at = new Date(wakes[0].ts);
  const name = `${at.toLocaleDateString("en-CA")}-${String(at.getHours()).padStart(2, "0")}${String(at.getMinutes()).padStart(2, "0")}`;
  const made: Recorded = {
    name,
    pages: map(root),
    functions: context?.functions,
    texts,
    start,
    held: { messages: ids("m"), updates: ids("u"), agents: [...start.matchAll(/^t\d+  a(\d+)  /gm)].map((found) => Number(found[1])) },
    entries: world.filter((entry) => entry.at <= ended),
    runs,
  };
  // The session is right when the same code that told Fast tells it the same again.
  const different = runs.filter((run) => told(made, run, roster(made.entries.filter((entry) => entry.n <= Math.max(...run.entries)))) !== run.wake).map((run) => run.run);
  mkdirSync(destination, { recursive: true });
  writeFileSync(join(destination, `${name}.json`), JSON.stringify(made, null, 1) + "\n");
  console.log(`${name}: ${runs.length} wake-ups, ${made.entries.length} entries of the world, ${Object.keys(texts).length} pages' texts.`);
  store.db.close();
  console.log(different.length === 0 ? "Every wake-up's text is made again exactly from the world." : `The text of wake-up ${different.join(", ")} is not made again exactly from the world: the recorded text is used.`);
  return made;
}

/** A recorded wake-up's page, as Fast saw it. */
function seen(session: Recorded, page: Run["page"]): Seen | null {
  return page && { ...page, text: session.texts[page.address] ?? "" };
}

/** What a recorded wake-up told Fast, made again from the world. */
function told(session: Recorded, run: Run, agents: string) {
  const entries = session.entries.filter((entry) => run.entries.includes(entry.n));
  const said = entries.filter((entry): entry is Message => entry.kind === "message" && run.said.includes(entry.id));
  return wakeText(run.run, [], entries, said, seen(session, run.page), run.unseen, agents);
}

// ---------------------------------------------------------------------------------------------------------------------
// A case's moment

/** What the model is sent for a case, and what its script runs in. */
interface Moment {
  instructions: string;
  input: any[];
  scope: Scope;
  registry?: Registry;
  /** Everything Fast was shown, as text: for the judge and for `show`. */
  shown: string;
}

/** A recorded script without the page line it wrote into a task: the page is added to every task now. */
const bare = (code: string) => code.replace(/(?:\\n|\n)Page: \$\{m\d+\.page\}/g, "");

/** A recording carries data-only function descriptions. No installed package is
 * loaded when evaluating an earlier session. */
function replayRegistry(functions: FastFunction[]) {
  const registry = new Registry(":memory:");
  const parts = Map.groupBy(functions, fn => fn.name.split(".")[0]);
  for (const [part, descriptions] of parts) {
    registry.part(part, ({ action }) => {
      for (const fn of descriptions) {
        const input = z.fromJSONSchema(fn.schema);
        if (!(input instanceof z.ZodObject)) throw new Error(`${fn.name} needs an object input`);
        action(fn.name.slice(part.length + 1), { for: "fast", description: fn.description, syntax: fn.syntax, input: input.shape }, () => {
          throw new Error("A recorded action cannot be executed during an eval");
        });
      }
    });
  }
  return registry;
}

export async function moment(test: Case, session = recorded(test.session)): Promise<Moment> {
  const registry = session.functions && replayRegistry(session.functions);
  session.start = session.start.replace(" When you cannot place what the user says, start a new agent.", "");
  for (const run of session.runs) for (const one of run.scripts) one.code = bare(one.code);
  // A line said at a wake-up that updates caused passes on what those agents reported.
  for (const run of session.runs) {
    const from = [...new Set(session.entries.filter((entry): entry is Update => entry.kind === "update" && entry.type !== "progress" && run.entries.includes(entry.n)).map((entry) => entry.agent))];
    const lines = run.scripts.at(-1)?.code.match(/\bsay\(/g)?.length ?? 0;
    if (run.said.length > 0 || from.length === 0 || lines === 0) continue;
    const spoken = session.entries.filter((entry): entry is Message => entry.kind === "message" && entry.who === "voice" && entry.n > Math.max(...run.entries)).slice(0, lines);
    for (const line of spoken) line.about = from;
  }
  const until = (n: number) => roster(session.entries.filter((entry) => entry.n <= n));
  const before = test.run !== undefined ? test.run - 1 : (test.after ?? session.runs.length);
  const input: any[] = [{ role: "user", content: session.start }];
  const shown: string[] = [session.start];
  const messages = new Map<number, Message>();
  const updates = new Map<number, Update>();
  const agents = new Set(session.held.agents);
  const hold = (entry: Entry) => {
    if (entry.kind === "message") messages.set(entry.id, entry);
    if (entry.kind === "update") updates.set(entry.id, entry);
    if (entry.kind === "update" || entry.kind === "task") agents.add(entry.agent);
  };
  for (const entry of session.entries) {
    if ((entry.kind === "message" && session.held.messages.includes(entry.id)) || (entry.kind === "update" && session.held.updates.includes(entry.id))) hold(entry);
  }
  let call = 0;
  const answered = (code: string, output: string) => {
    const id = `call_eval_${(call += 1)}`;
    input.push({ type: "function_call", call_id: id, name: "exec", arguments: JSON.stringify({ code }) }, { type: "function_call_output", call_id: id, output });
    shown.push(`[the script it answered with]\n${code}${output === "ok" ? "" : `\n[it threw: ${output}]`}`);
  };
  const wake = (text: string, images: string[] = []) => {
    const content = images.length ? [{ type: "input_text", text }, ...images.map(image_url => ({ type: "input_image", image_url }))] : text;
    input.push({ role: "user", content });
    shown.push(text);
  };
  const scope = (page: Seen | null, said: number[]): Scope => ({
    messages: [...messages.values()].map(({ id, text, page, marked }) => ({ id, text, page, marked })),
    updates: [...updates.values()].map(({ id, text }) => ({ id, text })),
    agents: [...agents],
    page: page && { address: page.address, text: page.text },
    pages: session.pages.map(({ address }) => address),
    handover: said,
    pads: page?.pads?.map(({ address }) => address) ?? [],
  });
  const done = (page: Seen | null, said: number[]): Moment => ({ instructions: instructions(session.pages, test.routes ?? "", registry?.brief()), registry, input, scope: scope(page, said), shown: shown.join("\n\n") });

  for (const run of session.runs.slice(0, before)) {
    for (const entry of session.entries) if (run.entries.includes(entry.n)) hold(entry);
    wake(session.functions !== undefined ? run.wake : told(session, run, until(Math.max(...run.entries))), run.images);
    for (const { code, output } of run.scripts) answered(code, output);
  }

  if (test.run !== undefined) {
    const run = session.runs[before];
    if (!run) throw new Error(`${test.name}: the session ${test.session} has no wake-up ${test.run}.`);
    for (const entry of session.entries) if (run.entries.includes(entry.n)) hold(entry);
    wake(session.functions !== undefined ? run.wake : told(session, run, until(Math.max(...run.entries))), run.images);
    return done(seen(session, run.page), run.said);
  }

  // A written moment. What the world had between the last recorded wake-up and the thing that woke the next one is
  // still to be shown: what Voice said aloud, what moved on the page.
  const last = session.runs[before - 1];
  const next = session.runs[before];
  const reached = last ? Math.max(...last.entries) : Math.max(0, ...session.entries.filter((entry) => entry.kind === "message" && session.held.messages.includes(entry.id)).map((entry) => entry.n));
  let waiting: Entry[] = [];
  for (const entry of session.entries.filter((entry) => entry.n > reached)) {
    if (next ? (entry.kind === "message" && next.said.includes(entry.id)) || ((entry.kind === "update" || entry.kind === "problem") && next.entries.includes(entry.n)) || entry.n > Math.max(...next.entries) : false) break;
    waiting.push(entry);
  }
  let state = { ...(next?.page ?? last?.page ?? { address: session.pages[0].address, marked: "", panel: false }) };
  let told_text = [...session.runs.slice(0, before)].reverse().find((run) => run.unseen)?.page?.address ?? null;
  // The written entries are numbered on from what the world had by then.
  const past = session.entries.filter((entry) => entry.n <= (waiting.at(-1)?.n ?? reached));
  const highest = (kind: Entry["kind"]) => Math.max(0, ...past.flatMap((entry) => (entry.kind === kind && "id" in entry ? [entry.id] : [])));
  let n = Math.max(0, ...past.map((entry) => entry.n));
  let message = highest("message");
  let update = highest("update");
  let task = highest("task");
  const newest = new Map<number, number>();
  for (const entry of past) if (entry.kind === "task") newest.set(entry.agent, entry.id);
  const all = [...past];
  const add = (entry: Record<string, unknown>) => {
    const full = { n: (n += 1), at: new Date(0).toISOString(), ...entry } as Entry;
    waiting.push(full);
    all.push(full);
    return full;
  };
  // The scratch pads, and the one on the canvas.
  const pads = new Map(Object.entries(test.pads ?? {}));
  let canvas: string | null = null;
  const steps = test.steps ?? [];
  if (steps.length === 0) throw new Error(`${test.name}: a case has a recorded wake-up (run) or written steps.`);
  for (const [index, step] of steps.entries()) {
    if (step.closed && canvas) {
      canvas = null;
      state = { ...state, marked: "" };
      add({ kind: "page", by: "me", text: "The user closed the canvas themselves." });
    }
    if (step.opened && step.opened !== state.address) {
      if (!(step.opened in session.texts)) throw new Error(`${test.name}: the session has no text of ${step.opened}. It has: ${Object.keys(session.texts).join(", ")}.`);
      state = { ...state, address: step.opened, marked: "" };
      add({ kind: "page", by: "me", text: `The user opened ${step.opened}.` });
    }
    if (step.marked !== undefined && step.marked !== state.marked) {
      state = { ...state, marked: step.marked };
      add({ kind: "page", by: "me", text: step.marked ? `The user marked ${JSON.stringify(step.marked)}.` : "The user cleared the mark." });
    }
    for (const given of step.updates ?? []) {
      const of = given.task ?? newest.get(given.agent);
      if (of === undefined) throw new Error(`${test.name}: a${given.agent} has no task to publish an update on.`);
      add({ kind: "update", id: (update += 1), task: of, agent: given.agent, type: given.type, text: given.text });
    }
    const said: Message[] = step.he === undefined ? [] : [add({ kind: "message", id: (message += 1), who: "me", text: step.he, page: canvas ?? state.address, marked: state.marked, start_ms: null, end_ms: null, handover: `written_${index}` }) as Message];
    for (const entry of waiting) hold(entry);
    const listed = [...pads].map(([address, { title }]) => ({ address, title }));
    // While the canvas is open, the pad on it is the user's page.
    const page: Seen = canvas ? { ...state, address: canvas, text: pads.get(canvas)?.text ?? "", canvas: true, pads: listed } : { ...state, text: session.texts[state.address] ?? "", ...(listed.length > 0 ? { pads: listed } : {}) };
    // A page the session has no text of is shown by its address alone.
    const unseen = told_text !== page.address && (canvas !== null || state.address in session.texts);
    if (unseen) told_text = page.address;
    wake(wakeText(before + index + 1, [], waiting, said, page, unseen, roster(all)));
    waiting = [];
    const ids = said.map(({ id }) => id);
    if (index === steps.length - 1) return done(page, ids);
    if (step.script === undefined) throw new Error(`${test.name}: step ${index + 1} needs the script Fast answered it with.`);
    // The script does to the world what the service would have done with it.
    const code = bare(step.script);
    const ran = await execute(code, scope(page, ids), registry).catch((error) => {
      throw new Error(`${test.name}: the script of step ${index + 1} does not run: ${error.message}`);
    });
    answered(code, "ok");
    const lines: string[] = [];
    const from = [...new Set((step.updates ?? []).filter((given) => given.type !== "progress").map((given) => given.agent))];
    for (const intent of ran.intents) {
      if (intent.do === "say") lines.push(intent.text);
      else if (intent.do === "restart") continue;
      else if (intent.do === "world") throw new Error(`The recorded eval has no implementation of ${intent.name}`);
      else if (intent.do === "spawn" || intent.do === "send" || intent.do === "fork") {
        const agent = intent.do === "send" ? intent.agent : Math.max(0, ...agents) + 1;
        agents.add(agent);
        newest.set(agent, (task += 1));
        add({ kind: "task", id: task, agent, how: intent.do, from: intent.do === "fork" ? intent.agent : null, prompt: intent.prompt });
        if (said.some((one) => one.marked)) state = { ...state, marked: "" };
      } else {
        if (intent.do === "show") {
          canvas = intent.address;
          if (!pads.has(intent.address)) pads.set(intent.address, { title: intent.title ?? intent.address, text: intent.title ?? "" });
          state = { ...state, marked: "" };
        }
        if (intent.do === "close" || intent.do === "go") canvas = null;
        if (intent.do === "go") state = { ...state, address: intent.address.split("#")[0], marked: "" };
        if (intent.do === "panel") state = { ...state, panel: intent.open };
        if (intent.do === "select") state = { ...state, marked: intent.quote };
        add({ kind: "page", by: "fast", text: moved(intent) });
      }
    }
    for (const text of lines) add({ kind: "message", id: (message += 1), who: "voice", text, page: canvas ?? state.address, marked: "", start_ms: null, end_ms: null, handover: null, ...(said.length === 0 && from.length > 0 ? { about: from } : {}) });
  }
  throw new Error("unreachable");
}

// ---------------------------------------------------------------------------------------------------------------------
// One try of a case

/** A model with its settings, as `model[:effort[:tier]]`. */
interface Setup {
  name: string;
  model: string;
  effort: string;
  tier: string;
}

function setup(text: string): Setup {
  const [model, effort = EFFORT, tier = "priority"] = text.split(":");
  return { name: `${model} ${effort} ${tier}`, model, effort, tier };
}

interface Try {
  /** What is wrong with it; empty when it passed. */
  wrong: string[];
  code: string;
  /** Scripts the model wrote: 2 when the first threw. */
  attempts: number;
  /** Until the script that ran was there. */
  ms: number;
  cost: number;
  judged: number;
}

const PROVIDER = provider(process.env);

async function post(body: unknown) {
  for (let attempt = 1; ; attempt += 1) {
    const response = await fetch(PROVIDER!.responses, {
      method: "POST",
      headers: { ...PROVIDER!.headers, "content-type": "application/json" },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(120_000),
    }).catch((error) => new Response(String(error), { status: 599 }));
    if (response.ok) return response.json() as Promise<any>;
    const text = await response.text();
    // A full or failing provider is tried again; anything else is the eval's own mistake.
    if (attempt === 4 || ![429, 500, 502, 503, 529, 599].includes(response.status)) throw new Error(`The API answered ${response.status}: ${text.slice(0, 400)}`);
    await Bun.sleep(1500 * attempt);
  }
}

const spent = (answer: any) => {
  const usage = answer.usage ?? {};
  return price(answer.model ?? "", answer.service_tier ?? null, { input_tokens: usage.input_tokens ?? null, cached_tokens: usage.input_tokens_details?.cached_tokens ?? null, output_tokens: usage.output_tokens ?? null }) ?? 0;
};

/** The words a call carries: a line, a prompt for an agent, or the part of the page it points at. */
const words = (intent: Intent) => (intent.do === "say" ? intent.text : "prompt" in intent ? intent.prompt : "quote" in intent ? intent.quote : "");

/** Whether a call is the one wanted. */
function fits(intent: Intent, want: Want) {
  const call = intent as Record<string, unknown>;
  if (![want.do].flat().includes(intent.do)) return false;
  if (want.agent !== undefined && call.agent !== want.agent) return false;
  // A route is wanted by its name; "" wants none, which is the default.
  if (want.route !== undefined && ![want.route].flat().includes(String(call.route ?? ""))) return false;
  if (want.address !== undefined && String(call.address).split("#")[0] !== want.address) return false;
  if (want.not !== undefined && String(call.address).split("#")[0] === want.not) return false;
  if (want.where !== undefined && call.where !== want.where) return false;
  if (want.open !== undefined && call.open !== want.open) return false;
  if (want.words !== undefined && words(intent).trim().split(/\s+/).filter(Boolean).length > want.words) return false;
  return (want.has ?? []).every((part) => words(intent).toLowerCase().includes(part.toLowerCase()));
}

const named = (want: Want) => `${[want.do].flat().join(" or ")}${want.agent !== undefined ? ` for a${want.agent}` : ""}${want.address ? ` to ${want.address}` : ""}${want.where ? ` ${want.where}` : ""}${want.open !== undefined ? ` ${want.open}` : ""}${want.has ? ` with ${want.has.map((part) => JSON.stringify(part)).join(", ")}` : ""}${want.words !== undefined ? ` of at most ${want.words} words` : ""}`;

/** What is wrong with a script's calls. */
function checked(expect: Expect, intents: Intent[]) {
  const wrong: string[] = [];
  const left = [...intents];
  for (const want of expect.does) {
    const at = left.findIndex((intent) => fits(intent, want));
    if (at < 0) wrong.push(`no ${named(want)}`);
    else left.splice(at, 1);
  }
  for (const intent of left) {
    if (!(expect.may ?? []).includes(intent.do)) wrong.push(`${intent.do}${"agent" in intent ? ` for a${intent.agent}` : ""} that should not be there`);
  }
  return wrong;
}

/** What every line and every prompt is held to, whatever the case. */
const ALWAYS = {
  line: ["It states as a fact only what the dispatcher was shown, in the entries, the page's text or an update. Nothing is invented, and it does not say that something was read, written, fixed or built unless an update says so. What the same script does may be said as being done: a page call (\"I've scrolled down\"), work given to an agent (\"I'm checking that\"). What the dispatcher's own instructions tell it, about itself and about how the assistant works, is known to it."],
  prompt: [
    "No more than the user asked: it does not ask for a change of files when the user asked a question, and it adds no piece of work the user did not ask for. Saying what the user means, naming the page or the part, quoting what the user agrees to, and asking the agent to report are not extra work. A scratch pad is not a file of the project: while the canvas is open on a pad, telling the agent to draw its answer there or to bring the pad up to date with what the user said is how a question about the pad's idea is answered, and is not a change the user did not ask for.",
    "No less than the user asked: the agent can do the work from this prompt, together with what that same agent did, asked and said before when the prompt is sent to it or to a fork of it. No part of what the user asked for is missing, and there is no \"that\", \"it\" or \"there\" the agent has no way to place.",
  ],
};

const judging = new Map<string, Promise<{ wrong: string[]; cost: number }>>();

let judges = 0;
const queue: (() => void)[] = [];

/** One question to the judge model, with no tools. A few run at once. */
async function graded(system: string, prompt: string) {
  if (judges >= AT_ONCE) await new Promise<void>((free) => queue.push(free));
  else judges += 1;
  try {
    for (let attempt = 1; ; attempt += 1) {
      let text = "";
      try {
        for await (const message of query({ prompt, options: { model: JUDGE, systemPrompt: system, allowedTools: [], maxTurns: 1, settingSources: [], persistSession: false } })) {
          if (message.type === "result" && message.subtype === "success") text = message.result;
        }
        JSON.parse(text.slice(text.indexOf("{"), text.lastIndexOf("}") + 1));
        return text;
      } catch (error) {
        if (attempt === 3) throw error;
      }
      if (attempt === 3) throw new Error("the judge gave no answer");
    }
  } finally {
    const next = queue.shift();
    if (next) next();
    else judges -= 1;
  }
}

/** Has the judge model say, point by point, whether the words hold. The same words in the same case are judged once. */
function judge(test: Case, at: Moment, code: string, what: "line" | "prompt", text: string) {
  const points = [...(test.expect[what] ?? []), ...ALWAYS[what]];
  const id = `${test.name}\n${what}\n${text}\n${code}`;
  let asked = judging.get(id);
  if (!asked) {
    asked = graded(
      "You grade one piece of an assistant's work. The user talks aloud to an assistant while reading a page of a project. A dispatcher gets what the user says and what the working agents report, and answers with a script: `say(...)` is spoken to the user, `spawn`, `aN.send` and `aN.fork` give a prompt to a working agent, `page...` moves the user's page. You are given the dispatcher's own instructions, then everything it was shown, then its whole script, then the words of it to grade, then numbered points. The script's calls are carried out: a page call in it moves the page, and work it gives to an agent is being done. For each point say whether it holds for those words. Go only by what you are given. A point fails only for a fault you can name in the words given, one that would change what the agent does or what the user is told; a doubt, or a wording you would have chosen differently, is not a fault. The working agents know the project, its files and its pages: a thing the user calls by its usual name needs no address, and the user's own words are never a fault of the prompt. Answer with JSON and nothing else: {\"points\": [{\"point\": 1, \"why\": \"one short sentence, written before you decide\", \"holds\": true}]}",
      `The dispatcher's instructions:\n\n${at.instructions}\n\n=====\n\nWhat the dispatcher was shown, oldest first:\n\n${at.shown}\n\n=====\n\nThe script the dispatcher answered the last wake-up with:\n\n${code}\n\n=====\n\n${what === "line" ? "The line that script has spoken to the user, with its variables filled in" : "The prompt that script gives to a working agent, with its variables filled in"}:\n\n${text}\n\n=====\n\nThe points:\n${points.map((point, index) => `${index + 1}. ${point}`).join("\n")}`,
    ).then((written) => {
      const grades: { point: number; holds: boolean; why: string }[] = JSON.parse(written.slice(written.indexOf("{"), written.lastIndexOf("}") + 1)).points;
      const wrong = points.flatMap((point, index) => {
        const grade = grades.find((one) => one.point === index + 1);
        return grade?.holds ? [] : [`${what}: "${point}" does not hold: ${grade?.why ?? "the judge gave no grade"}`];
      });
      return { wrong, cost: 0 };
    });
    judging.set(id, asked);
  }
  return asked;
}

async function attempt(test: Case, at: Moment, with_: Setup): Promise<Try> {
  const input = [...at.input];
  let ms = 0;
  let cost = 0;
  let attempts = 0;
  const written = await script(
    input,
    at.scope,
    async () => {
      const began = performance.now();
      const answer = await post(request(with_.model, { instructions: at.instructions, input }, { tool_choice: { type: "function", name: "exec" }, reasoning: { effort: with_.effort }, service_tier: with_.tier, prompt_cache_key: `live-eval-${with_.name}` }));
      ms += performance.now() - began;
      cost += spent(answer);
      attempts += 1;
      return answer;
    },
    () => {},
    at.registry,
  );
  if (!written) return { wrong: ["no script that ran"], code: "", attempts, ms, cost, judged: 0 };
  const intents = written.ran.intents;
  const wrong = checked(test.expect, intents);
  let judged = 0;
  for (const intent of intents) {
    const what = intent.do === "say" ? "line" : "prompt" in intent ? "prompt" : null;
    if (!what) continue;
    const grade = await judge(test, at, written.code, what, words(intent));
    wrong.push(...grade.wrong);
    judged += grade.cost;
  }
  return { wrong, code: written.code, attempts, ms, cost, judged };
}

// ---------------------------------------------------------------------------------------------------------------------
// The command

/** Runs jobs a few at a time, and keeps their order. */
async function all<T>(jobs: (() => Promise<T>)[]) {
  const results: T[] = new Array(jobs.length);
  let next = 0;
  await Promise.all(
    Array.from({ length: AT_ONCE }, async () => {
      while (next < jobs.length) {
        const mine = next++;
        results[mine] = await jobs[mine]();
      }
    }),
  );
  return results;
}

const seconds = (ms: number) => `${(ms / 1000).toFixed(1)} s`;
const of = (values: number[], share: number) => [...values].sort((a, b) => a - b)[Math.min(values.length - 1, Math.floor(values.length * share))];

/** A case that is kept out of an everyday run. */
const HELD = (one: Case) => one.sets?.includes("held-out") === true;

/** The names of the cases that did not pass every time in the last run that was saved. */
function lastFailed() {
  const folder = RUNS;
  const last = existsSync(folder) ? readdirSync(folder).filter((name) => name.endsWith(".json")).sort().at(-1) : undefined;
  if (!last) throw new Error("No run was saved yet: there is nothing that failed.");
  const saved = JSON.parse(readFileSync(join(folder, last), "utf8")) as { runs: number; report: { cases: { name: string; passed: number }[] }[] };
  const names = new Set(saved.report.flatMap((setup) => setup.cases.filter((one) => one.passed < saved.runs).map((one) => one.name)));
  console.log(`The last run, ${last}: ${names.size} cases did not pass every time.`);
  return names;
}

async function main() {
  const args = process.argv.slice(2);
  if (args[0] === "sets") {
    for (const [name, about] of Object.entries(SETS)) console.log(`${name.padEnd(12)} ${String(CASES.filter((one) => one.sets?.includes(name)).length).padStart(3)}  ${about}`);
    console.log(`${"all".padEnd(12)} ${String(CASES.filter((one) => !HELD(one)).length).padStart(3)}  Every set but held-out: what a run without --set is.`);
    return;
  }
  if (args[0] === "freeze") return freeze(args[1] ?? "");
  if (args[0] === "show") {
    const test = CASES.find((one) => one.name === args[1]);
    if (!test) throw new Error(`No case ${args[1]}. The cases: ${CASES.map((one) => one.name).join(", ")}`);
    const at = await moment(test);
    console.log(at.shown);
    console.log(`\n[expected] ${JSON.stringify(test.expect, null, 1)}`);
    return;
  }
  const option = (name: string) => {
    const at = args.indexOf(name);
    return at < 0 ? null : args.splice(at, 2)[1];
  };
  const runs = Number(option("--runs") ?? 3);
  const only = option("--only");
  const sets = option("--set")?.split(",").map((name) => name.trim()).filter(Boolean) ?? null;
  const failed = args.includes("--failed") ? (args.splice(args.indexOf("--failed"), 1), lastFailed()) : null;
  const list = args.includes("--list") ? (args.splice(args.indexOf("--list"), 1), true) : false;
  // What is left is the setups to run. Something that looks like an option is a mistake, not a model to call.
  const strange = args.find((arg) => arg.startsWith("-") || /\s/.test(arg));
  if (strange) throw new Error(`${JSON.stringify(strange)} is not an option. The options: --set, --only, --failed, --runs, --list.`);
  const setups = (args.length > 0 ? args : [BACKEND]).map(setup);
  for (const name of sets ?? []) if (name !== "all" && !(name in SETS)) throw new Error(`No set ${name}. The sets: ${Object.keys(SETS).join(", ")}, and all for every set but held-out.`);
  const cases = CASES.filter((one) => {
    if (only && !one.name.includes(only)) return false;
    if (failed && !failed.has(one.name)) return false;
    // A set that is named is run. With none named, a held-out case runs only when it is asked for by its name or as one that failed.
    if (sets) return sets.some((name) => (name === "all" ? !HELD(one) : one.sets?.includes(name)));
    return only || failed ? true : !HELD(one);
  });
  if (failed && failed.size === 0) return console.log("Every case passed every time in the last run that was saved: nothing to run again.");
  if (cases.length === 0) throw new Error(`No case fits: ${[sets && `the sets ${sets.join(", ")}`, only && `"${only}" in its name`, failed && "did not pass in the last run"].filter(Boolean).join(", ")}.`);
  console.log(`${cases.length} of ${CASES.length} cases, ${runs} times each, with ${setups.length === 1 ? "one setup" : `${setups.length} setups`}: ${cases.length * runs * setups.length} calls of the model.`);
  if (list) return console.log(cases.map((one) => `  ${(one.sets ?? []).join(", ").padEnd(12)} ${one.name}`).join("\n"));
  if (!PROVIDER) throw new Error(`${NEEDS} is not in the environment.`);
  const moments = new Map<string, Moment>();
  for (const test of cases) moments.set(test.name, await moment(test));

  const report: Record<string, unknown>[] = [];
  for (const with_ of setups) {
    const tries = await all(cases.flatMap((test) => Array.from({ length: runs }, () => () => attempt(test, moments.get(test.name)!, with_).catch((error): Try => ({ wrong: [`the eval failed: ${error.stack ?? error.message}`], code: "", attempts: 0, ms: 0, cost: 0, judged: 0 })))));
    const by = cases.map((test, index) => ({ test, tries: tries.slice(index * runs, index * runs + runs) }));
    const passes = (one: (typeof by)[number]) => one.tries.filter((made) => made.wrong.length === 0).length;
    const times = tries.filter((made) => made.attempts > 0).map((made) => made.ms);
    console.log(`\n${with_.name}: ${cases.length} cases, ${runs} times each`);
    console.log(`  Passed every time: ${by.filter((one) => passes(one) === runs).length}. Sometimes: ${by.filter((one) => passes(one) > 0 && passes(one) < runs).length}. Never: ${by.filter((one) => passes(one) === 0).length}. Tries passed: ${tries.filter((made) => made.wrong.length === 0).length} of ${tries.length}.`);
    console.log(`  Time to the script: ${seconds(of(times, 0.5))} typically, ${seconds(of(times, 0.9))} for the slowest tenth. A second script was needed ${tries.filter((made) => made.attempts > 1).length} times.`);
    console.log(`  Cost: $${tries.reduce((sum, made) => sum + made.cost, 0).toFixed(3)} for the model. The judge runs on the subscription.`);
    for (const one of by.filter((one) => passes(one) < runs)) {
      console.log(`\n  ${one.test.name}: passed ${passes(one)} of ${runs}`);
      const shown = new Set<string>();
      for (const made of one.tries.filter((made) => made.wrong.length > 0)) {
        const text = `${made.wrong.map((line) => `    - ${line}`).join("\n")}\n      ${made.code.replace(/\s+/g, " ").slice(0, 600)}`;
        if (!shown.has(made.wrong.join()) && shown.size < 2) console.log(text);
        shown.add(made.wrong.join());
      }
    }
    report.push({ setup: with_, cases: by.map((one) => ({ name: one.test.name, passed: passes(one), tries: one.tries })) });
  }
  const folder = RUNS;
  mkdirSync(folder, { recursive: true });
  const file = join(folder, `${new Date().toISOString().replace(/[:.]/g, "-")}.json`);
  writeFileSync(file, JSON.stringify({ runs, report }, null, 1));
  console.log(`\nEvery try, with its script and what was wrong with it: ${file}`);
}

if (import.meta.main) await main();
