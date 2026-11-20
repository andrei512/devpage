// Fast: the hub between Voice and the slow agents, and its harness. Fast is a model that is woken when the user said
// something and when agents published updates. Each time it gets the world's new entries and answers with one script,
// which runs in the sandbox; what the script asked for is then carried out: a line for Voice, work for an agent, a
// call of a part's function. The harness keeps Fast's conversation itself, so it can build a successor with a short context
// beside it and swap the two between wake-ups, with no wait for the user.
import { run as execute, type Intent, type Ran, type Scope } from "./sandbox.ts";
import { line, type Entry, type Message, type Update, type World } from "./world.ts";
import type { Task } from "./tasks.ts";
import type { Registry, Shown } from "./registry.ts";
import { coreEntry } from "./events.ts";
import { price } from "./cost.ts";
import type { Provider } from "./provider.ts";
import { assemble, type TextOf } from "./prompt.ts";

/** The model, how hard it thinks, and the model a session goes on with when the API refuses the first. On Azure these name deployments. */
export const BACKEND = "gpt-6-luna";
export const EFFORT = "medium";
export const FALLBACK = "gpt-6.1-sol";

/** Fast's context, in tokens, at which a successor is built. */
const SOFT_LIMIT = 200_000;
/** The context at which the swap no longer waits for a quiet moment. */
const HARD_LIMIT = 250_000;
/** What a successor starts with. */
const RECENT_MESSAGES = 30;
const RECENT_TASKS = 12;
/** How long a prefix is taken to stay in the provider's cache. Not checked. */
const CACHE_LIFE = 4 * 60_000;
/** How long updates are collected before Fast is woken for them: several that end together are one wake-up. */
const GATHER = 400;
/** What a script's calls are carried out with. */
export interface Carried {
  run: number;
  /** The hand-over the run answers, when it has one. */
  handover: string | null;
  /** The user's messages of that hand-over. */
  said: Message[];
  /** The messages and updates the script read. */
  used: string[];
}

export interface FastDeps {
  world: World;
  registry: Registry;
  /** What Fast is told about the routes a new agent can run on, with the user's rules. Empty when there is only one route. */
  routes?: () => string;
  routeNames?: () => string[];
  /** The persisted work snapshot also used by the panel. */
  work: () => WorkState;
  /** Carries out one thing a script asked for. Throws when it cannot be done. Returns what there is to log. */
  carry: (intent: Intent, with_: Carried) => Promise<unknown>;
  /** The agents Fast no longer holds may be closed. */
  forget: (keep: Set<number>) => void;
  /** Whether the user is silent right now. */
  quiet: () => boolean;
  log: (kind: string, fields: { run?: number; text?: string; [more: string]: unknown }) => void;
}

const TOOLS = [
  {
    type: "function",
    name: "exec",
    description: "Runs one script in JavaScript. The script's calls are carried out when it ends. Returns \"ok\", or the error when it threw: then nothing was done.",
    parameters: { type: "object", properties: { code: { type: "string", description: "The script" } }, required: ["code"], additionalProperties: false },
    strict: true,
  },
];

/** Fast's prompt as its pieces: the template, the text of the parts, the list of their functions and the routes. */
export function pieces(registry: Registry, routes = "") {
  const functions = { from: `made from functions: ${[...new Set(registry.fastFunctions().filter((fn) => !fn.syntax).map((fn) => fn.name.split(".")[0]))].join(", ")}`, text: registry.brief() };
  return assemble("fast", registry.texts(), { functions, routes: { from: "routing", text: routes } });
}

/** What the model is told first. */
export function instructions(registry: Registry, routes = "") {
  return pieces(registry, routes).map((piece) => piece.text).join("");
}

/** What the parts show Fast, as the pieces of a wake-up: each title with its text, only where the text differs from what this context was last shown under that title. */
export function showing(last: Map<string, string>, shown: Shown) {
  const pieces: string[] = [];
  for (const [part, titles] of Object.entries(shown)) {
    for (const [title, text] of Object.entries(titles)) {
      const key = `${part}\n${title}`;
      if ((last.get(key) ?? "") === text) continue;
      last.set(key, text);
      if (text) pieces.push(`${title}:\n${text}`);
    }
  }
  return pieces;
}

const short = (text: string, most: number) => { const flat = text.replace(/\s+/g, " ").trim(); return flat.length > most ? `${flat.slice(0, most)}…` : flat; };

export interface WorkState {
  tasks: Task[];
  agents: { id: number; state: "working" | "asked" | "idle"; from: number | string | null }[];
}

/** Fast sees precisely the agents and tasks the current work snapshot contains. */
function workRoster({ tasks, agents }: WorkState) {
  if (!agents.length) return "The agents: none yet.";
  const lines = ["The agents:"];
  for (const agent of agents) {
    const assigned = tasks.filter(task => task.agent === agent.id);
    const last = assigned.at(-1);
    const waiting = last?.latest?.type === "question" ? `waiting for the user's answer to u${last.latest.id}` : "waiting for the user's answer";
    const status = agent.state === "asked" ? waiting : agent.state === "working" ? "busy" : "free";
    lines.push(`a${agent.id}  ${status}${typeof agent.from === "number" ? `  (started with what a${agent.from} knew)` : ""}`);
    for (const task of assigned.slice(-3).reverse()) {
      const state = task.state === "done" ? "finished" : task.state === "running" ? "busy" : "failed";
      lines.push(`  t${task.id}  ${state}  ${JSON.stringify(short(task.request, 160))}${task.summary ? "  " + short(task.summary, 160) : ""}`);
    }
  }
  return lines.join("\n");
}

/** The agents as they are, from the world's tasks and updates: each one's state, and its last tasks as they were given, with what it asked or last reported. */
export function roster(entries: Entry[]) {
  const agents = new Map<number, { from: number | null; tasks: { id: number; words: string; state: string; note: string }[] }>();
  for (const entry of entries) {
    if (entry.kind === "task") {
      const agent = agents.get(entry.agent) ?? { from: entry.from, tasks: [] };
      const words = short(entry.prompt, 160);
      agent.tasks.push({ id: entry.id, words, state: "busy", note: "" });
      agents.set(entry.agent, agent);
    }
    if (entry.kind === "update") {
      const task = agents.get(entry.agent)?.tasks.find((one) => one.id === entry.task);
      if (task) task.note = entry.type === "question" ? `it asked: ${JSON.stringify(short(entry.text, 160))}` : entry.type === "progress" ? `last progress: ${JSON.stringify(short(entry.text, 160))}` : "";
      if (task) task.state = entry.type === "question" ? `waiting for the user's answer to u${entry.id}` : entry.type === "progress" ? "busy" : entry.type;
    }
  }
  if (agents.size === 0) return "The agents: none yet.";
  const lines = ["The agents:"];
  for (const [id, agent] of agents) {
    const open = agent.tasks.filter((task) => task.state === "busy" || task.state.startsWith("waiting"));
    const state = open.some((task) => task.state === "busy") ? "busy" : open.length > 0 ? open.at(-1)!.state : "free";
    lines.push(`a${id}  ${state}${agent.from ? `  (started with what a${agent.from} knew)` : ""}`);
    for (const task of agent.tasks.slice(-3).reverse()) lines.push(`  t${task.id}  ${task.state.startsWith("waiting") ? "asked the user something" : task.state}  ${JSON.stringify(task.words)}${task.note ? `  ${task.note}` : ""}`);
  }
  return lines.join("\n");
}

export type ShownEntry = (Entry | { n: number; kind: "registered" }) & { rendered?: string };

/** What Fast is told at a wake-up: what woke it, the entries, what the parts show that Fast has not seen, and the agents. */
export function wakeText(run: number, before: ShownEntry[], fresh: ShownEntry[], said: Message[], shown: string[], agents: string) {
  const lines: string[] = [];
  const shownLine = (entry: ShownEntry) => entry.rendered ?? (entry.kind === "registered" ? "" : line(entry));
  const updates = fresh.filter((entry) => entry.kind === "update");
  const problems = fresh.filter((entry) => entry.kind === "problem");
  const woke = [
    said.length > 0 ? `The user just said ${said.map((message) => `m${message.id}`).join(", ")}.` : "",
    updates.length > 0 ? `New ${updates.length === 1 ? "update" : "updates"}: ${updates.map((entry) => `u${(entry as Update).id}`).join(", ")}.` : "",
    problems.length > 0 ? "Something could not be done." : "",
  ].filter(Boolean);
  lines.push(`Wake-up ${run}. ${woke.join(" ") || "Nothing new was said."}`);
  if (before.length > 0) lines.push("", "Already handled, before your context was built:", ...before.map(shownLine));
  if (fresh.length > 0) lines.push("", ...fresh.map(shownLine));
  for (const piece of shown) lines.push("", piece);
  lines.push("", agents);
  return lines.join("\n");
}

/** One request to the model: a conversation, and what this call adds to the usual settings. */
export function request(model: string, context: { instructions: string; input: any[] }, more: Record<string, unknown>) {
  return {
    model,
    instructions: context.instructions,
    input: context.input,
    tools: TOOLS,
    parallel_tool_calls: false,
    store: false,
    // The model's own reasoning comes back encrypted and is sent again with the conversation: nothing is kept at the provider.
    include: ["reasoning.encrypted_content"],
    reasoning: { effort: EFFORT },
    prompt_cache_key: "live-fast",
    // The priority tier answers in about 1.4 s where the default one takes 2 to 2.6 s.
    service_tier: "priority",
    ...more,
  };
}

/** A script the model wrote, and what running it gave. */
export interface Written {
  code: string;
  ran: Ran;
  /** 1, or 2 when the first script threw. */
  attempt: number;
}

/**
 * A wake-up's script: the model is asked for one and it runs in the sandbox. When it threw, the model gets the error
 * and is asked once more. The model's answers and what became of them are added to `input`. Null when no script ran.
 */
export async function script(input: any[], scope: Scope, ask: (attempt: number) => Promise<any | null>, note: (kind: string, fields: Record<string, unknown>) => void, registry?: Registry, shown: Shown = {}): Promise<Written | null> {
  for (let attempt = 1; attempt <= 2; attempt += 1) {
    const response = await ask(attempt);
    if (!response) return null;
    input.push(...response.output);
    const call = response.output.find((item: any) => item.type === "function_call");
    if (!call) {
      note("no_script", { attempt, output: response.output });
      return null;
    }
    let code = "";
    try {
      code = String(JSON.parse(call.arguments).code ?? "");
      const ran = await execute(code, scope, registry, shown);
      input.push({ type: "function_call_output", call_id: call.call_id, output: "ok" });
      return { code, ran, attempt };
    } catch (error) {
      const why = error instanceof Error ? error.message : String(error);
      input.push({ type: "function_call_output", call_id: call.call_id, output: `${why}\nNothing was done.${attempt === 1 ? " Write the script again." : ""}` });
      note("script_failed", { attempt, text: code || call.arguments, error: why });
    }
  }
  return null;
}

/** A conversation of Fast's: what the model is sent, and what of the world is in it. */
interface Context {
  instructions: string;
  input: any[];
  /** The world's newest entry that is in it. */
  cursor: number;
  messages: Map<number, Message>;
  updates: Map<number, Update>;
  agents: Set<number>;
  /** What the parts last showed in it, by part and title. */
  shown: Map<string, string>;
  /** When the model last read it: its prefix is taken to be cached for a while after. */
  touched: number;
  /** The context's size at the last call, in tokens. */
  tokens: number;
  /** Entries up to this number were handled by the context before it. */
  handled: number;
}

export class Fast {
  private context: Context;
  private successor: Context | null = null;
  private preparing = false;
  private running = false;
  private runs = 0;
  private model: string;
  private timer: ReturnType<typeof setTimeout> | null = null;
  /** The hand-overs that wait for a run. */
  private handovers: { id: string; messages: Message[] }[] = [];
  private reasons: string[] = [];
  private wakeCursor = 0;
  /** Whether a voice session is on: Fast runs only then, because only then can the user hear it. */
  private listening = false;

  constructor(private readonly provider: Provider, model: string, private readonly deps: FastDeps) {
    this.model = model;
    this.context = this.build(null);
    deps.log("context_built", { text: this.context.input[0].content, why: "start", cursor: this.context.cursor, functions: deps.registry.fastFunctions() });
  }

  /** A voice session started or ended. When one starts, Fast gets what happened while none was on. */
  listen(on: boolean) {
    this.listening = on;
    if (!on) return;
    void this.warm(this.context, "voice on");
    this.wake("voice on");
  }

  /** The newest committed entry whose kind asked for a run. */
  entry(why: string, n: number) {
    this.wakeCursor = Math.max(this.wakeCursor, n);
    this.wake(why);
  }

  /** Wakes Fast: for a hand-over, with the user's messages it came with, or because the world has something new. */
  wake(why: string, handover?: { id: string; messages: Message[] }) {
    this.reasons.push(why);
    if (handover) this.handovers.push(handover);
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => void this.pump(), this.handovers.length > 0 ? 0 : GATHER);
  }

  private async pump() {
    if (this.running || !this.listening) return;
    this.running = true;
    try {
      while (this.listening && this.due()) await this.run();
    } catch (error) {
      this.deps.log("run_error", { text: error instanceof Error ? (error.stack ?? error.message) : String(error) });
    } finally {
      this.running = false;
    }
    this.swap();
  }

  /** Whether there is something to wake Fast for: a hand-over, an update or a problem it has not seen. */
  private due() {
    return this.handovers.length > 0 || this.wakeCursor > Math.max(this.context.cursor, this.context.handled);
  }

  /** One wake-up: the new entries in, one script out, its calls carried out. */
  private async run() {
    const run = ++this.runs;
    const began = Date.now();
    const context = this.context;
    const handovers = this.handovers.splice(0);
    const reasons = this.reasons.splice(0);
    const registry = this.deps.registry;
    const events = registry.since(context.cursor);
    const entries: ShownEntry[] = events.map(event => ({
      ...(coreEntry(event) ?? { kind: "registered" as const, n: event.n }), rendered: registry.line(event),
    }));
    const shown = await registry.shown();
    const said = handovers.flatMap((handover) => handover.messages);
    const text = this.render(context, run, entries, said, shown);
    if (entries.length > 0) context.cursor = entries.at(-1)!.n;
    context.input.push(registry.wake(events, text));
    this.deps.log("wake", { run, text, why: reasons, handovers: handovers.map((handover) => handover.id), entries: entries.map((entry) => entry.n), shown_ms: Date.now() - began });

    const carried: Carried = { run, handover: handovers.at(-1)?.id ?? null, said, used: [] };
    const activeAgents = new Set(this.deps.work().agents.map(agent => agent.id));
    const scope: Scope = {
      messages: [...context.messages.values()].map(({ id, text, page, marked }) => ({ id, text, page, marked })),
      updates: [...context.updates.values()].map(({ id, text }) => ({ id, text })),
      agents: [...context.agents].filter((id) => activeAgents.has(id)),
      routes: this.deps.routeNames?.(),
    };
    const written = await script(
      context.input,
      scope,
      (attempt) => this.call(context, { run, attempt, what: "script" }, { tool_choice: { type: "function", name: "exec" } }),
      (kind, fields) => this.deps.log(kind, { run, ...fields }),
      registry,
      shown,
    );
    if (written) {
      const { code, ran, attempt } = written;
      this.deps.log("script", { run, attempt, text: code, intents: ran.intents, used: ran.used, printed: ran.printed, sandbox_ms: ran.ms, ms: Date.now() - began });
      carried.used = ran.used;
      await this.carry(ran.intents, carried);
      this.deps.log("run_ended", { run, ms: Date.now() - began, did: ran.intents.map((intent) => intent.do) });
      return;
    }
    // No script that ran: the user is told, when the user is waiting for something.
    this.deps.log("run_failed", { run, ms: Date.now() - began });
    if (said.length > 0) await this.carry([{ do: "say", text: "I couldn't do that, something went wrong on my side." }], carried);
  }

  /** Carries out a script's calls, in their order. One that fails becomes a problem in the world, and the rest still run. */
  private async carry(intents: Intent[], carried: Carried) {
    for (const intent of intents) {
      const began = Date.now();
      try {
        const result = await this.deps.carry(intent, carried);
        this.deps.log("did", { run: carried.run, intent, result, ms: Date.now() - began });
      } catch (error) {
        const why = error instanceof Error ? error.message : String(error);
        this.deps.log("did_not", { run: carried.run, intent, error: why, ms: Date.now() - began });
        this.deps.world.add({ kind: "problem", text: `${describe(intent)} could not be done: ${why}` });
      }
    }
  }

  /** Takes a wake-up's entries into the context, so that everything shown is a variable, and returns what Fast is told. */
  private render(context: Context, run: number, entries: ShownEntry[], said: Message[], shown: Shown) {
    for (const entry of entries) if (entry.kind !== "registered") this.hold(context, entry);
    const before = entries.filter((entry) => entry.n <= context.handled);
    const fresh = entries.filter((entry) => entry.n > context.handled);
    context.handled = 0;
    return wakeText(run, before, fresh, said, showing(context.shown, shown), workRoster(this.deps.work()));
  }

  /** Takes an entry into what a context holds: its id is a variable from now on. */
  private hold(context: Context, entry: Entry) {
    if (entry.kind === "message") context.messages.set(entry.id, entry);
    if (entry.kind === "update") context.updates.set(entry.id, entry);
    if (entry.kind === "update" || entry.kind === "task") context.agents.add(entry.agent);
  }

  /** A new context from the world as it is: the recent messages, the recent tasks with their agents, and a summary of what was before when there is one. */
  private build(summary: string | null): Context {
    const world = this.deps.world;
    const context: Context = { instructions: "", input: [], cursor: this.deps.registry.events.now, messages: new Map(), updates: new Map(), agents: new Set(), shown: new Map(), touched: 0, tokens: 0, handled: 0 };
    context.instructions = instructions(this.deps.registry, this.deps.routes?.() ?? "");
    const lines = ["This is the start of your context. The session may have been running before it."];
    if (summary) lines.push("", "A summary of the conversation before:", summary);
    const messages = world.recent("message", RECENT_MESSAGES);
    if (messages.length > 0) lines.push("", "The most recent messages:", ...messages.map(line));
    const work = this.deps.work();
    const tasks = work.tasks.slice(-RECENT_TASKS);
    const given = new Map(world.recent("task", 200).map((entry) => [entry.id, entry]));
    const shown: string[] = [];
    for (const task of tasks) {
      const entry = given.get(task.id);
      // A task from before the agents, or one whose agent is gone, cannot be followed up: it is left out.
      if (!entry || !work.agents.some(agent => agent.id === entry.agent)) continue;
      shown.push(`t${task.id}  a${entry.agent}  ${task.state}  ${JSON.stringify(task.title)}`, `  ${line(entry)}`);
      this.hold(context, entry);
      for (const update of world.updates(task.id).slice(-2)) {
        shown.push(`  ${line(update)}`);
        this.hold(context, update);
      }
    }
    if (shown.length > 0) lines.push("", "The recent tasks, each with the agent that has it and its newest updates:", ...shown);
    for (const message of messages) this.hold(context, message);
    lines.push("", "Older messages, tasks and agents are forgotten.");
    context.input.push({ role: "user", content: lines.join("\n") });
    return context;
  }

  /** Sends a context to the model once, with no work to do, so that the provider has its prefix cached when it is needed. */
  private async warm(context: Context, why: string) {
    if (Date.now() - context.touched < CACHE_LIFE) return true;
    const response = await this.call(context, { what: "boot", why }, { tool_choice: "none", max_output_tokens: 16 });
    return Boolean(response);
  }

  /** After a run: builds a successor when the context has grown past the soft limit, and swaps when one is ready. */
  private swap() {
    if (this.successor) return this.exchange();
    if (this.preparing || this.context.tokens < SOFT_LIMIT) return;
    this.preparing = true;
    void this.prepare().finally(() => (this.preparing = false));
  }

  private async prepare() {
    const began = Date.now();
    const old = this.context;
    this.deps.log("swap_started", { text: `The context has ${old.tokens} tokens: a successor is built.`, tokens: old.tokens, soft_limit: SOFT_LIMIT });
    // The summary is written by the model from the conversation it has, as it stood now. Without one the swap still happens.
    const asked = await this.call(
      { ...old, input: [...old.input, { role: "user", content: "Your context is about to be replaced by a new one. Write the summary your successor starts with, as plain text and not as a script: what the user is working on, what was decided, what the user likes and dislikes in how the user is answered, and anything still open between you. It gets the recent messages and the recent tasks separately, so leave those out. At most 300 words." }] },
      { what: "summary" },
      { tool_choice: "none" },
    );
    const summary = asked ? written(asked.output) : null;
    this.deps.log("swap_summary", { text: summary ?? "(none: the swap goes on without it)", ms: Date.now() - began });
    const next = this.build(summary);
    await this.warm(next, "successor");
    this.successor = next;
    this.deps.log("swap_ready", { text: next.input[0].content, cursor: next.cursor, ms: Date.now() - began });
    this.exchange();
  }

  /** Puts the successor in Fast's place, between two wake-ups and while the user is silent. Past the hard limit it no longer waits for silence. */
  private exchange() {
    const next = this.successor;
    if (!next || this.running) return;
    if (this.due()) return void this.pump();
    if (!this.deps.quiet() && this.context.tokens < HARD_LIMIT) return void setTimeout(() => this.exchange(), 1000);
    if (Date.now() - next.touched > CACHE_LIFE) {
      // Its prefix has left the cache while it waited: it is sent once more, and the swap follows.
      this.successor = null;
      this.preparing = true;
      return void this.warm(next, "successor again").finally(() => {
        this.preparing = false;
        this.successor = next;
        this.exchange();
      });
    }
    const old = this.context;
    // What the old context handled after the successor was built is shown to the successor as already handled.
    next.handled = old.cursor;
    this.context = next;
    this.successor = null;
    this.deps.forget(new Set(next.agents));
    this.deps.log("swap_done", { text: `Fast's context was swapped: ${old.tokens} tokens before.`, old_tokens: old.tokens, since_entry: next.cursor, handled_to: next.handled, agents: [...next.agents] });
  }

  /** One call of the model with a context. Null when it failed: the failure is in the log. */
  private async call(context: Pick<Context, "instructions" | "input"> & Partial<Context>, about: Record<string, unknown>, more: Record<string, unknown>): Promise<any | null> {
    const began = Date.now();
    const ask = (model: string) =>
      fetch(this.provider.responses, {
        method: "POST",
        headers: { ...this.provider.headers, "content-type": "application/json" },
        body: JSON.stringify(request(model, context, more)),
        signal: AbortSignal.timeout(45_000),
      });
    try {
      let response = await ask(this.model);
      if (!response.ok && this.model !== FALLBACK && [400, 403, 404].includes(response.status)) {
        const refused = await response.text();
        // Only a refusal of the model itself moves the session to the other one.
        if (/model/i.test(refused)) {
          this.deps.log("model_fallback", { ...about, text: refused.slice(0, 600), refused: this.model, fallback: FALLBACK });
          this.model = FALLBACK;
          response = await ask(this.model);
        } else {
          this.deps.log("model_error", { ...about, status: response.status, text: refused.slice(0, 2000), ms: Date.now() - began });
          return null;
        }
      }
      if (!response.ok) {
        this.deps.log("model_error", { ...about, status: response.status, text: (await response.text()).slice(0, 2000), ms: Date.now() - began });
        return null;
      }
      const answer = await response.json();
      const usage = answer.usage ?? {};
      if ("touched" in context) {
        context.touched = Date.now();
        context.tokens = (usage.input_tokens ?? 0) + (usage.output_tokens ?? 0);
      }
      this.deps.log("model", {
        ...about,
        level: "debug",
        model: answer.model ?? this.model,
        ms: Date.now() - began,
        status: answer.status,
        input_tokens: usage.input_tokens ?? null,
        cached_tokens: usage.input_tokens_details?.cached_tokens ?? null,
        output_tokens: usage.output_tokens ?? null,
        reasoning_tokens: usage.output_tokens_details?.reasoning_tokens ?? null,
        tier: answer.service_tier ?? null,
        cost_usd: price(answer.model ?? this.model, answer.service_tier ?? null, { input_tokens: usage.input_tokens ?? null, cached_tokens: usage.input_tokens_details?.cached_tokens ?? null, output_tokens: usage.output_tokens ?? null }),
      });
      return answer;
    } catch (error) {
      this.deps.log("model_error", { ...about, text: error instanceof Error ? error.message : String(error), ms: Date.now() - began });
      return null;
    }
  }
}

/** The text a model wrote, from a response's output. */
function written(output: any[]) {
  const text = output.flatMap((item) => (item.type === "message" ? item.content : [])).map((part: any) => part?.text).filter((part: unknown) => typeof part === "string").join("").trim();
  return text || null;
}

/** A call, in words, for a problem Fast is told about. */
function describe(intent: Intent) {
  switch (intent.do) {
    case "world":
      return intent.name;
    case "say":
      return "Saying a line";
    case "spawn":
      return "Starting a new agent";
    case "send":
      return `Giving a${intent.agent} more work`;
    case "fork":
      return `Forking a${intent.agent}`;
    case "restart":
      return "Restarting";
  }
}
