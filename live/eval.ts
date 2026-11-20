// The evals of Fast. A case is one moment of a session: what Fast had been shown until then, the wake-up, and what a
// right script does there. The moment is put to the model through the same code the service runs (the prompt, the
// parts the program brings, the request, the sandbox, the second try), several times, and each script is checked: its
// calls in code, and the words of a spoken line or of a prompt for an agent by a judge model.
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
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { z } from "zod";
import { BACKEND, EFFORT, instructions, request, roster, script, showing, wakeText, type ShownEntry } from "./fast.ts";
import { registerCore } from "./core-parts.ts";
import { canvas as canvasPart } from "./parts/canvas/part.ts";
import { devpage as devpagePart } from "./parts/devpage/part.ts";
import { Registry, type FastFunction, type Shown } from "./registry.ts";
import { SessionStore } from "./store.ts";
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

/** A call by its name: one of Fast's own (`say`, `spawn`, `send`, `fork`, `restart`), or a part's function, such as `devpage.go`. */
type Kind = string;

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
  /** The page the user was on then, as the `devpage` part had it. */
  page: { address: string; marked: string; panel: boolean } | null;
  /** What Fast was told, as it was. */
  wake: string;
  images?: string[];
  /** What it answered: one script, or two when the first threw. */
  scripts: { code: string; output: string }[];
}

/** Something that happened on the user's page, in a written session: the line of it Fast read. */
interface PageLine {
  n: number;
  at: string;
  kind: "page";
  text: string;
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
  entries: (Entry | PageLine)[];
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
  // What a wake-up's text says of the page is read back from it: the log has the text Fast was told and no more. The
  // page is told only when it changed, so it is kept from one wake-up to the next.
  let page: Run["page"] = null;
  const runs: Run[] = wakes.map((wake) => {
    const text: string = wake.text;
    const on = /^The user is on (\S+)\. (?:Marked now: (".*")|Nothing is marked)\. The side panel is (open|closed)\.$/m.exec(text);
    if (on) page = { address: on[1], marked: on[2] ? JSON.parse(on[2]) : "", panel: on[3] === "open" };
    else if (/^No page of the user's is open/m.test(text)) page = null;
    const read = /\n<<<\n([\s\S]*?)\n>>>(?:\n|$)/.exec(text);
    if (read && page) texts[page.address] = read[1];
    const images = [...store.events.images(wake.entries).values()];
    return {
      run: wake.run,
      entries: wake.entries,
      said: [...(/The user just said ((?:m\d+(?:, )?)+)\./.exec(text.split("\n")[0])?.[1] ?? "").matchAll(/m(\d+)/g)].map((found) => Number(found[1])),
      page: page && { ...page },
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
  mkdirSync(destination, { recursive: true });
  writeFileSync(join(destination, `${name}.json`), JSON.stringify(made, null, 1) + "\n");
  console.log(`${name}: ${runs.length} wake-ups, ${made.entries.length} entries of the world, ${Object.keys(texts).length} pages' texts.`);
  store.db.close();
  return made;
}

// ---------------------------------------------------------------------------------------------------------------------
// A case's moment

/** What the model is sent for a case, and what its script runs in. */
interface Moment {
  instructions: string;
  input: any[];
  scope: Scope;
  registry: Registry;
  /** What the parts showed Fast at the moment: what a call of its script is checked against. */
  parts: Shown;
  /** Everything Fast was shown, as text: for the judge and for `show`. */
  shown: string;
}

/** A recorded script without the page line it wrote into a task: the page is added to every task now. */
const bare = (code: string) => code.replace(/(?:\\n|\n)Page: \$\{m\d+\.page\}/g, "");

/**
 * The world a case happens in: the registry with the core's parts and the two parts the program brings, as the service
 * has them. Only what is outside the program is made up: the user's tab, which shows the session's pages with the texts
 * the session has, and a folder of scratch pads. A recording's functions of other parts are checked and never run: no
 * installed package is loaded for an eval.
 */
function stage(session: Recorded, test: Case) {
  const registry = new Registry(":memory:");
  registerCore(registry);
  const tab = { address: session.pages[0]?.address ?? "/", panel: false };
  const folder = mkdtempSync(join(tmpdir(), "devpage-eval-pads-"));
  // The pads are listed newest first: in a case, in the order they are written.
  for (const [index, [address, pad]] of Object.entries(test.pads ?? {}).entries()) {
    const file = join(folder, address.slice("pads/".length));
    writeFileSync(file, `# ${pad.title}\n`);
    utimesSync(file, new Date(Date.now() - index * 60_000), new Date(Date.now() - index * 60_000));
  }
  const pad = () => (registry.state("canvas").canvas as { pad: string } | null)?.pad ?? null;
  /** What the user reads: the pad on the canvas, or the page. A pad a script made has its title alone, and so has a page the session has no text of. */
  const read = () => {
    const on = pad();
    if (on) return test.pads?.[on]?.text ?? /^#\s+(.+)$/m.exec(readFileSync(join(folder, on.slice("pads/".length)), "utf8"))?.[1] ?? "";
    return session.texts[tab.address] ?? session.pages.find((page) => page.address === tab.address)?.title ?? "";
  };
  const page = devpagePart(registry, {
    ask: async (action) => {
      switch (action.name) {
        case "read": return { address: pad() ?? tab.address, text: read(), panel: tab.panel ? "open" : "closed" };
        case "go": tab.address = String(action.address).split("#")[0]; return { address: tab.address };
        case "panel": tab.panel = action.open === true; return { panel: tab.panel ? "open" : "closed" };
        case "find": return { part: { page: pad() ?? tab.address, quote: String(action.quote) } };
        default: return {};
      }
    },
    tab: () => ({ address: tab.address, sources: [tab.address] }),
    pages: () => session.pages,
    comment: async () => {},
    marked: () => null,
  });
  const canvas = canvasPart(registry, page, { folder, log: () => {} });
  const brought = registry.parts();
  for (const [part, functions] of Map.groupBy((session.functions ?? []).filter((fn) => !brought.includes(fn.name.split(".")[0])), (fn) => fn.name.split(".")[0])) {
    void registry.part(part, {
      functions: Object.fromEntries(functions.map((fn) => {
        const input = z.fromJSONSchema(fn.schema);
        if (!(input instanceof z.ZodObject)) throw new Error(`${fn.name} needs an object input`);
        return [fn.name.slice(part.length + 1), { for: "fast" as const, description: fn.description, input: input.shape, run: () => { throw new Error("a recorded function cannot be carried out in an eval"); } }];
      })),
    });
  }
  return {
    registry,
    page,
    canvas,
    /** The user is on this page, with this marked and the side panel so: what a recorded wake-up had. */
    at: (was: NonNullable<Run["page"]>) => {
      tab.address = was.address;
      tab.panel = was.panel;
      registry.step("devpage", ({ state }) => {
        state.address = was.address;
        state.subject = was.marked ? { page: was.address, quote: was.marked } : null;
        state.by = was.marked ? "user" : null;
        state.panel = was.panel;
      });
    },
    /** The user opened another page themselves. */
    opened: (address: string) => {
      tab.address = address;
      page.settle();
    },
    address: () => tab.address,
    marked: () => (registry.state("devpage").subject as { quote: string } | null)?.quote ?? "",
  };
}

export async function moment(test: Case, session = recorded(test.session)): Promise<Moment> {
  const { registry, page, canvas, ...user } = stage(session, test);
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
  const core = (entries: (Entry | PageLine)[]) => entries.filter((entry): entry is Entry => entry.kind !== "page");
  /** An entry as Fast is shown it: a part's entry is its line. */
  const entry = (one: Entry | PageLine): ShownEntry => (one.kind === "page" ? { n: one.n, kind: "registered", rendered: `page  ${one.text}` } : one);
  const until = (n: number) => roster(core(session.entries).filter((entry) => entry.n <= n));
  const before = test.run !== undefined ? test.run - 1 : (test.after ?? session.runs.length);
  const input: any[] = [{ role: "user", content: session.start }];
  const shown: string[] = [session.start];
  const messages = new Map<number, Message>();
  const updates = new Map<number, Update>();
  const agents = new Set(session.held.agents);
  const hold = (entry: Entry | PageLine | ShownEntry) => {
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
  const scope = (): Scope => ({
    messages: [...messages.values()].map(({ id, text, page, marked }) => ({ id, text, page, marked })),
    updates: [...updates.values()].map(({ id, text }) => ({ id, text })),
    agents: [...agents],
  });
  /** What Fast was last shown of each part: a part's text is told again only when it changed. */
  const told = new Map<string, string>();
  const done = (parts: Shown): Moment => {
    canvas.end();
    return { instructions: instructions(registry, test.routes ?? ""), registry, input, scope: scope(), parts, shown: shown.join("\n\n") };
  };
  /** A recorded wake-up: the page is as it was then, and the parts show it. A recording with functions of other parts is told as it was told. */
  const again = async (run: Run) => {
    const entries = session.entries.filter((entry) => run.entries.includes(entry.n));
    for (const entry of entries) hold(entry);
    if (run.page) user.at(run.page);
    const parts = await registry.shown();
    const pieces = showing(told, parts);
    const said = core(entries).filter((entry): entry is Message => entry.kind === "message" && run.said.includes(entry.id));
    wake(session.functions !== undefined ? run.wake : wakeText(run.run, [], entries.map(entry), said, pieces, until(Math.max(...run.entries))), run.images);
    return parts;
  };

  for (const run of session.runs.slice(0, before)) {
    await again(run);
    for (const { code, output } of run.scripts) answered(code, output);
  }

  if (test.run !== undefined) {
    const run = session.runs[before];
    if (!run) throw new Error(`${test.name}: the session ${test.session} has no wake-up ${test.run}.`);
    return done(await again(run));
  }

  // A written moment. What the world had between the last recorded wake-up and the thing that woke the next one is
  // still to be shown: what Voice said aloud, what moved on the page.
  const last = session.runs[before - 1];
  const next = session.runs[before];
  const reached = last ? Math.max(...last.entries) : Math.max(0, ...session.entries.filter((entry) => entry.kind === "message" && session.held.messages.includes(entry.id)).map((entry) => entry.n));
  let waiting: ShownEntry[] = [];
  for (const one of session.entries.filter((entry) => entry.n > reached)) {
    if (next ? (one.kind === "message" && next.said.includes(one.id)) || ((one.kind === "update" || one.kind === "problem") && next.entries.includes(one.n)) || one.n > Math.max(...next.entries) : false) break;
    waiting.push(entry(one));
  }
  // The page is as the last script left it.
  const start = next?.page ?? last?.page;
  if (start) user.at(start);
  // The written entries are numbered on from what the world had by then.
  const past = core(session.entries).filter((entry) => entry.n <= (waiting.at(-1)?.n ?? reached));
  const highest = (kind: Entry["kind"]) => Math.max(0, ...past.flatMap((entry) => (entry.kind === kind && "id" in entry ? [entry.id] : [])));
  let n = Math.max(0, ...session.entries.filter((entry) => entry.n <= (waiting.at(-1)?.n ?? reached)).map((entry) => entry.n));
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
  /** What the parts added since, as their lines, in the order of everything else. */
  let cursor = registry.events.now;
  const parts = () => {
    for (const event of registry.since(cursor)) {
      waiting.push({ n: (n += 1), kind: "registered", rendered: registry.line(event) });
      cursor = event.n;
    }
  };
  const steps = test.steps ?? [];
  if (steps.length === 0) throw new Error(`${test.name}: a case has a recorded wake-up (run) or written steps.`);
  for (const [index, step] of steps.entries()) {
    const run = before + index + 1;
    if (step.closed) canvas.closed();
    if (step.opened && step.opened !== user.address()) {
      if (!(step.opened in session.texts)) throw new Error(`${test.name}: the session has no text of ${step.opened}. It has: ${Object.keys(session.texts).join(", ")}.`);
      user.opened(step.opened);
    }
    if (step.marked !== undefined && step.marked !== user.marked()) page.marked(step.marked ? { page: page.stamp().page ?? user.address(), quote: step.marked } : null);
    parts();
    for (const given of step.updates ?? []) {
      const of = given.task ?? newest.get(given.agent);
      if (of === undefined) throw new Error(`${test.name}: a${given.agent} has no task to publish an update on.`);
      add({ kind: "update", id: (update += 1), task: of, agent: given.agent, type: given.type, text: given.text });
    }
    // A message is stored with where the user is and what is marked, as the service stores it.
    const now = page.stamp();
    const said: Message[] = step.he === undefined ? [] : [add({ kind: "message", id: (message += 1), who: "me", text: step.he, page: now.page, marked: now.part?.quote ?? "", start_ms: null, end_ms: null, handover: `written_${index}` }) as Message];
    parts();
    for (const entry of waiting) hold(entry);
    const showed = await registry.shown();
    wake(wakeText(run, [], waiting, said, showing(told, showed), roster(all)));
    waiting = [];
    if (index === steps.length - 1) return done(showed);
    if (step.script === undefined) throw new Error(`${test.name}: step ${index + 1} needs the script Fast answered it with.`);
    // The script does to the world what the service does with it: a part's call is carried out by its part.
    const code = bare(step.script);
    const ran = await execute(code, scope(), registry, showed).catch((error) => {
      throw new Error(`${test.name}: the script of step ${index + 1} does not run: ${error.message}`);
    });
    answered(code, "ok");
    const lines: string[] = [];
    const from = [...new Set((step.updates ?? []).filter((given) => given.type !== "progress").map((given) => given.agent))];
    for (const intent of ran.intents) {
      if (intent.do === "say") lines.push(intent.text);
      else if (intent.do === "restart") continue;
      else if (intent.do === "world") {
        await registry.call(intent.name, intent.input, { who: "fast", run, handover: null, used: ran.used }).catch((error) => {
          throw new Error(`${test.name}: ${intent.name} of step ${index + 1} could not be carried out: ${error.message}`);
        });
        parts();
      } else {
        const agent = intent.do === "send" ? intent.agent : Math.max(0, ...agents) + 1;
        agents.add(agent);
        newest.set(agent, (task += 1));
        add({ kind: "task", id: task, agent, how: intent.do, from: intent.do === "fork" ? intent.agent : null, prompt: intent.prompt });
        // The mark goes with the task.
        if (now.part && said.length > 0) page.gave(now.part);
      }
    }
    // The tab reports the page it shows now.
    page.settle();
    parts();
    const where = page.stamp().page;
    for (const text of lines) add({ kind: "message", id: (message += 1), who: "voice", text, page: where, marked: "", start_ms: null, end_ms: null, handover: null, ...(said.length === 0 && from.length > 0 ? { about: from } : {}) });
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

/** A call's name: Fast's own by its word, a part's function by its full name. */
const kind = (intent: Intent) => (intent.do === "world" ? intent.name : intent.do);
/** What a call was given. */
const given = (intent: Intent): Record<string, unknown> => (intent.do === "world" ? intent.input : intent);

/** The words a call carries: a line, a prompt for an agent, or the part of the page it points at. */
const words = (intent: Intent) => (intent.do === "say" ? intent.text : "prompt" in intent ? intent.prompt : String(given(intent).quote ?? ""));

/** A part's function that only answers something, as `canvas.pad` answers a pad's address: it is never a call too many. */
const ANSWERS = new Set(["canvas.pad"]);

/** Whether a call is the one wanted. */
function fits(intent: Intent, want: Want) {
  const call = given(intent);
  if (![want.do].flat().includes(kind(intent))) return false;
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
    if (!(expect.may ?? []).includes(kind(intent)) && !ANSWERS.has(kind(intent))) wrong.push(`${kind(intent)}${"agent" in intent ? ` for a${intent.agent}` : ""} that should not be there`);
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

/** The judge's grades, one for each point, in the points' order: by the number it gave each, or by their order when it gave none. Null when a point has no grade. */
function grades(written: string, count: number): { holds: boolean; why: string }[] | null {
  let given: { point?: number; holds?: unknown; why?: unknown }[];
  try {
    given = JSON.parse(written.slice(written.indexOf("{"), written.lastIndexOf("}") + 1)).points;
  } catch {
    return null;
  }
  if (!Array.isArray(given)) return null;
  const numbered = given.every((one) => typeof one.point === "number");
  const found = Array.from({ length: count }, (_, index) => (numbered ? given.find((one) => one.point === index + 1) : given.length === count ? given[index] : undefined));
  if (found.some((one) => typeof one?.holds !== "boolean")) return null;
  return found.map((one) => ({ holds: one!.holds as boolean, why: String(one!.why ?? "") }));
}

/** One question to the judge model, with no tools, asked again until it grades every point. A few run at once. */
async function graded(system: string, prompt: string, count: number) {
  if (judges >= AT_ONCE) await new Promise<void>((free) => queue.push(free));
  else judges += 1;
  try {
    for (let attempt = 1; ; attempt += 1) {
      let text = "";
      try {
        for await (const message of query({ prompt, options: { model: JUDGE, systemPrompt: system, allowedTools: [], maxTurns: 1, settingSources: [], persistSession: false } })) {
          if (message.type === "result" && message.subtype === "success") text = message.result;
        }
        const all = grades(text, count);
        if (all) return all;
      } catch (error) {
        if (attempt === 3) throw error;
      }
      if (attempt === 3) throw new Error("the judge did not grade every point, three times");
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
      "You grade one piece of an assistant's work. The user talks aloud to an assistant while reading a page of a project. A dispatcher gets what the user says and what the working agents report, and answers with a script: `say(...)` is spoken to the user, `spawn`, `aN.send` and `aN.fork` give a prompt to a working agent, a call such as `devpage.go(...)` or `canvas.show(...)` changes what the user's page shows. You are given the dispatcher's own instructions, then everything it was shown, then its whole script, then the words of it to grade, then numbered points. The script's calls are carried out: a page call in it moves the page, and work it gives to an agent is being done. For each point say whether it holds for those words. Go only by what you are given. A point fails only for a fault you can name in the words given, one that would change what the agent does or what the user is told; a doubt, or a wording you would have chosen differently, is not a fault. The working agents know the project, its files and its pages: a thing the user calls by its usual name needs no address, and the user's own words are never a fault of the prompt. Answer with JSON and nothing else: {\"points\": [{\"point\": 1, \"why\": \"one short sentence, written before you decide\", \"holds\": true}]}",
      `The dispatcher's instructions:\n\n${at.instructions}\n\n=====\n\nWhat the dispatcher was shown, oldest first:\n\n${at.shown}\n\n=====\n\nThe script the dispatcher answered the last wake-up with:\n\n${code}\n\n=====\n\n${what === "line" ? "The line that script has spoken to the user, with its variables filled in" : "The prompt that script gives to a working agent, with its variables filled in"}:\n\n${text}\n\n=====\n\nThe points:\n${points.map((point, index) => `${index + 1}. ${point}`).join("\n")}`,
      points.length,
    ).then((given) => {
      const wrong = points.flatMap((point, index) => (given[index].holds ? [] : [`${what}: "${point}" does not hold: ${given[index].why}`]));
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
    at.parts,
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
