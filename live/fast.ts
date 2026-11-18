// Fast: the hub between Voice and the slow agents, and its harness. Fast is a model that is woken when the user said
// something and when agents published updates. Each time it gets the world's new entries and answers with one script,
// which runs in the sandbox; what the script asked for is then carried out: a line for Voice, work for an agent, a
// move of the user's page. The harness keeps Fast's conversation itself, so it can build a successor with a short context
// beside it and swap the two between wake-ups, with no wait for the user.
import { run as execute, type Intent, type Ran, type Scope } from "./sandbox.ts";
import { line, type Entry, type Message, type Update, type World } from "./world.ts";
import type { Task } from "./tasks.ts";
import { price } from "./cost.ts";
import type { Provider } from "./provider.ts";

/** The model, how hard it thinks, and the model a session goes on with when the API refuses the first. On Azure these name deployments. */
export const BACKEND = "gpt-6-luna";
const EFFORT = "medium";
export const FALLBACK = "gpt-6.1-sol";

/** Fast's context, in tokens, at which a successor is built. */
const SOFT_LIMIT = 60_000;
/** The context at which the swap no longer waits for a quiet moment. */
const HARD_LIMIT = 150_000;
/** What a successor starts with. */
const RECENT_MESSAGES = 30;
const RECENT_TASKS = 12;
/** How long a prefix is taken to stay in the provider's cache. Not checked. */
const CACHE_LIFE = 4 * 60_000;
/** How long updates are collected before Fast is woken for them: several that end together are one wake-up. */
const GATHER = 400;
/** The most of a page's text Fast is shown. */
const PAGE_TEXT = 14_000;

/** The page the user is on, as Fast is shown it. */
export interface Seen {
  address: string;
  text: string;
  /** The text that is marked now; empty when nothing is. */
  marked: string;
  panel: boolean;
  /** The canvas is open, and the address and the text are those of the scratch pad on it. */
  canvas?: boolean;
  /** The scratch pads there are, each with its idea's title. */
  pads?: { address: string; title: string }[];
}

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
  /** The page the user is on; null when no page is open or it does not answer. */
  page: () => Promise<Seen | null>;
  /** Every page's address and title. */
  pages: () => { address: string; title: string }[];
  /** What Fast is told about the routes a new agent can run on, with the user's rules. Empty when there is only one route. */
  routes?: () => string;
  tasks: () => Task[];
  /** Whether an agent still exists. */
  agent: (id: number) => boolean;
  /** Carries out one thing a script asked for. Throws when it cannot be done. Returns what there is to log. */
  carry: (intent: Intent, with_: Carried) => Promise<unknown>;
  /** The agents Fast no longer holds may be closed. */
  forget: (keep: Set<number>) => void;
  /** Whether the user is silent right now. */
  quiet: () => boolean;
  log: (kind: string, fields: { run?: number; text?: string; [more: string]: unknown }) => void;
}

const FAST_BRIEF = `The user is working on a project. The user talks aloud while reading a page of it in the browser, and marks the part they mean. To the user there is one assistant, a thinking partner and doing partner. It is made of three: Voice hears and speaks to the user; you, Fast, decide what happens with what the user says; the slow agents read and change the project and report with updates.

You are woken when the user said something and when an agent published an update. You answer with exactly one call of \`exec\`: a script. Text outside it reaches nobody. You decide in about a second and do no work yourself.

You know only what you are shown: what the user and Voice said, the user's page with its text and the part the user marked, the list of pages, the agents with their tasks and updates. Not the project's files or code, not the browser beyond that page. When the user asks what you can see or do, that is the answer.

# The script

\`\`\`js
m41, m41.page, m41.marked   // something the user said: in a string it is the user's words; the page and the marked text then
u12                         // an agent's update: in a string it is its text
spawn(task)                 // a new agent gets the task you write
a2.send(task)               // that agent gets it, when it is free
a2.fork(task)               // a new agent that starts with what a2 knows gets it
say(text)                   // Voice says it to the user
page.go(address)            // an address from the list of pages, with an optional #heading
page.scroll(where)          // "up", "down", "top", "bottom"
page.panel(open)            // the side panel with the tasks: true opens, false closes
page.point(quote)           // scrolls to the part of the user's page with these words and highlights it
page.select(quote)          // marks that part
canvas.pad(title)           // the address of the scratch pad for an idea, from its title in a few words
canvas.show(address)        // opens the canvas on that scratch pad, over the user's page
canvas.close()              // closes the canvas: the user's page is there as the user left it
restart()                   // when the user says "restart"; restart(true) only after "restart now"
\`\`\`

The calls are carried out when the script ends. A script that throws does nothing, and you get the error and one more try. Only ids you were shown exist. A quote is a few words copied exactly from the page's text, enough to be there once. Nothing to do: \`// nothing\`.

A task is read by an agent that did not hear the conversation: a new one knows nothing of it, and one you send to knows only its own tasks and answers. Write the task so that it can be done from the task alone:
- The user's words, exact, as \`\${m41}\`. When the user said it in pieces, all the pieces.
- What the user's words leave out, in a sentence or two of yours: what "that", "it" or "there" is, which page or part when it is not the one the user is on, what you or an agent said that the user is agreeing to.
- When one message holds work for two agents, each task says which part is that agent's.
- Nothing the user did not ask for: not how to do it, no extra work, no change when the user asked a question.
- It speaks of the user and to the agent: "you" is the agent. You are not in it.
The page the user is on and the part the user has marked are added to every task for you.

\`\`\`js
// a2 said the cause is probably the cache, and the user says "then fix it"
a2.send(\`The user said: "\${m44}"\\nThe user means what the cache does wrong.\`);
say("On it.");
\`\`\`
\`\`\`js
// the user said "in the intro, the second paragraph" (m45), stopped, and now says "cut its last sentence" (m46)
spawn(\`The user said: "\${m45}" and then: "\${m46}"\`);
say("On it.");
\`\`\`

# What the user says

One message can hold several things; each gets its call.
- About the user's page: \`page\`. A page the user names that is not in the list: say that it is not there.
- The answer is a fact you can read off what you were shown, what the page says, what was said or reported, what is running: \`say\` it.
- The answer has to be weighed, not read: what do you think, is this good, is it too long, should we, which is better, was that right. That is a judgment, and it is not yours to give, even when the thing is in front of you and even when the user says "you": an agent, and one short line such as "Let me think about that". The agent that did or said the thing the user asks about, when there is one. No opinion of yours, not a first impression either. The task is a question: the user wants a view, and nothing changed.
- A fact and a judgment in one message: \`say\` the fact, and the judgment goes to an agent.
- Anything else, a question that needs reading or work that changes something: an agent, and one short line such as "I'm checking that". No part of an answer, no guess, and nothing said as done: "I'm stopping it", not "stopped".
- The user asks again for what an agent is already doing, or how it is going: say that it is still being done, and what its last progress said. No new task.
- Half a sentence, thinking aloud, or words that are plainly not for you: nothing. "Stop" or "enough" while an answer is being spoken is for the talking: nothing.
Never state as a fact what you were not shown, and never say something was done unless an update said so.

# The canvas

Some things cannot be said well. Then you show them: the canvas is a large sheet over the user's page that shows one scratch pad, a page that is not in the project, which an agent draws and the user looks at, points at and talks about. You do not write a pad. You decide that there is one, open the canvas on it, and give an agent the task of drawing on it.

Open it in two cases:
- The user asks for it: "show me", "draw it", "put it on the scratch pad", "let's think about this together", "open the canvas".
- The answer would be something the user cannot keep in mind while hearing it, or would take longer than half a minute to say: parts and how they connect (a flow, a structure, who talks to whom), several options compared on several points, a plan with steps, exact things such as names, numbers or wording, or an idea you two build up over several turns. Then the agent gets the task of drawing it on a pad, and the user hears one short line.

Keep it closed, and say it or let an agent answer in words, for a yes or no, one fact, a short answer, an opinion, a change to the project, and how a task is going. That is for when no pad is open: once the canvas is open on an idea, what the user says about that idea is handled as below. When a page of the project already shows what the user asks for, go to that page and point: no pad.

\`\`\`js
// the user says "show me how the three agents work together" (m50): a new idea, so a new pad
const pad = canvas.pad("How the three agents work together");
canvas.show(pad);
spawn(\`The user said: "\${m50}"\nDraw the answer on the scratch pad \${pad}, which the user is looking at now.\`);
say("I'm drawing it.");
\`\`\`

- A new idea gets a new pad: \`canvas.pad\` with the idea's title, then \`canvas.show\`, then the task, which names the pad's address. An idea that has a pad in the list of scratch pads is shown again with \`canvas.show\` of that address.
- While the canvas is open, the pad is the user's page: you are shown its text, a part the user marks is a part of it. The pad's address is added to the task for you. No new pad while the user is still on the same idea.
- While the canvas is open, the pad is where the idea is worked out. Whatever the user says about its idea goes to the agent that draws it, as a follow-up that tells it to bring the pad up to date with it: a correction, an addition, a new requirement, and also "does that make sense" and "what do you think" about something the user adds. The user looks at the pad for the answer and hears a sentence or two. A pad that stays as it was while the user hears a long answer is the failure to avoid. The judgment is still the agent's: your own line says only that it is being worked into the pad, with no "that makes sense", no "I think" and no "I like it too".
- "Design only", "just research", "no changes", "don't build it yet" are about the project's files, never about the pad. Do not tell the agent to leave the pad as it is, and do not tell it to answer only in words.
- When the user moves to another idea that needs showing, it gets its own pad, and you say which idea is on the canvas now.
- "Close it", "go back", "back to the page": \`canvas.close()\`. \`page.go\` closes it too.
- Close it yourself when its idea is done: the user agrees with what is on the pad and asks for it to be built, written into the project or done ("I like this, implement it", "good, write that up"), or the user moves on to work that needs no picture. Then \`canvas.close()\` and the work go in the same script. Not when the next thing is another idea to show: then \`canvas.show\` of the new pad takes the old one's place, with no close. Agreeing while the user keeps working on the idea is not done.
- When the user closed the canvas themselves, it stays closed: what the user says next is answered in words, however long, until the user asks to be shown something again.
- A pad is empty until the agent has drawn on it: say that it is being drawn, never that it is there or what it shows. When the user asks to see it while it is still being drawn ("show me", "where is it"), say that it is still being drawn. Not that you are opening it: it is open. The agent gets a follow-up only when the user added something to the work.
- When the agent drew or changed the pad and the canvas is open, the user reads the pad: say one or two sentences from its update, what is on the pad now and what the user has to decide, also when the update is long. Its doubts stay in. Never read a pad aloud, and add nothing from the pad's text.

# Which agent

1. Does an agent already know what this is about? It does when the user continues what that agent did or said, however long ago: the user goes on from its answer, answers its question, asks more about it, changes or takes back its work, says "that" or "it" about it. A line of Voice's marked \`(a2's answer)\` is what a2 reported: what the user says right after it is often about it, but only when the user's words fit it. If no agent knows it: \`spawn\`. A free agent that worked on the very thing the user now names may get it too.
2. Is that agent free, or waiting for the user's answer? \`send\`. Busy: \`send\` when the user changes the work it is doing now (a correction, an addition, "stop"), \`fork\` when it is work of its own.

# What the agents report

- An update about the pad that is open on the canvas is the one exception to "as it is": never \`say(u12)\` for it when it is longer than two sentences. Write one or two sentences of your own from the update alone: that it is on the pad now, and the one thing the user has to decide. No walk through its parts or steps, and nothing taken from the pad's text: the user reads the rest on the pad. An update of one or two sentences is said as it is.
- \`finished\`, \`question\`, \`failed\`: \`say(u12)\`, as it is. The agent's "probably" and "I did not check" are part of the answer. Only when it is far too long to listen to, say less of it, with its doubts. When you answer later from what an update said, it is still only as sure as the update was.
- Several at once: one \`say\`. An update that comes together with the user's words is still said, after what the user's words need.
- \`progress\`: nothing. It is what you answer from when the user asks how it is going.
- A \`problem\`: one line on what could not be done, or do it another way.

# How you talk

One or two plain spoken sentences, as "I". Never mention agents, tasks, models or handing anything over. No repeating the user, no praise, no offers. A question only when you cannot go on without the user's answer.`;
const TOOLS = [
  {
    type: "function",
    name: "exec",
    description: "Runs one script in JavaScript. The script's calls are carried out when it ends. Returns \"ok\", or the error when it threw: then nothing was done.",
    parameters: { type: "object", properties: { code: { type: "string", description: "The script" } }, required: ["code"], additionalProperties: false },
    strict: true,
  },
];

/** What the model is told first: the brief, and every page's address with its title. */
export function instructions(pages: { address: string; title: string }[], routes = "") {
  return `${FAST_BRIEF}${routes ? `\n\n${routes}` : ""}\n\n# The pages\n\nEach as its address and its title:\n${pages.map(({ address, title }) => `${address} — ${title}`).join("\n")}`;
}

const short = (text: string, most: number) => { const flat = text.replace(/\s+/g, " ").trim(); return flat.length > most ? `${flat.slice(0, most)}…` : flat; };

/** The agents as they are, from the world's tasks and updates: each one's state, and its last tasks as they were given, with what it asked or last reported. */
export function roster(entries: Entry[]) {
  const agents = new Map<number, { from: number | null; tasks: { id: number; words: string; state: string; note: string }[] }>();
  for (const entry of entries) {
    if (entry.kind === "task") {
      const agent = agents.get(entry.agent) ?? { from: entry.from, tasks: [] };
      const words = short(entry.prompt.split("\n\nThe page the user is on:")[0], 160);
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

/** What Fast is told at a wake-up: what woke it, the entries, the page, with its text when Fast has not seen it, and the agents. */
export function wakeText(run: number, before: Entry[], fresh: Entry[], said: Message[], page: Seen | null, unseen: boolean, agents: string) {
  const lines: string[] = [];
  const updates = fresh.filter((entry) => entry.kind === "update");
  const problems = fresh.filter((entry) => entry.kind === "problem");
  const woke = [
    said.length > 0 ? `The user just said ${said.map((message) => `m${message.id}`).join(", ")}.` : "",
    updates.length > 0 ? `New ${updates.length === 1 ? "update" : "updates"}: ${updates.map((entry) => `u${(entry as Update).id}`).join(", ")}.` : "",
    problems.length > 0 ? "Something could not be done." : "",
  ].filter(Boolean);
  lines.push(`Wake-up ${run}. ${woke.join(" ") || "Nothing new was said."}`);
  if (before.length > 0) lines.push("", "Already handled, before your context was built:", ...before.map(line));
  if (fresh.length > 0) lines.push("", ...fresh.map(line));
  lines.push("");
  if (!page) {
    lines.push("No page of the user's is open, or it does not answer.");
  } else {
    lines.push(`${page.canvas ? "The canvas is open: the user is looking at the scratch pad" : "The user is on"} ${page.address}. ${page.marked ? `Marked now: ${JSON.stringify(page.marked)}.` : "Nothing is marked."} The side panel is ${page.panel ? "open" : "closed"}.`);
    if (unseen) {
      const cut = page.text.length > PAGE_TEXT;
      lines.push(`The ${page.canvas ? "scratch pad" : "page"}'s text${cut ? `, its first ${PAGE_TEXT} characters` : ""}:`, "<<<", cut ? page.text.slice(0, PAGE_TEXT) : page.text, ">>>");
    }
  }
  if (page?.pads?.length) lines.push("", "The scratch pads:", ...page.pads.map(({ address, title }) => `${address} — ${title}`));
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
export async function script(input: any[], scope: Scope, ask: (attempt: number) => Promise<any | null>, note: (kind: string, fields: Record<string, unknown>) => void): Promise<Written | null> {
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
      const ran = await execute(code, scope);
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

/** A page call of a script's, as the sentence the world keeps of it. */
export function moved(intent: Extract<Intent, { do: "go" | "scroll" | "panel" | "point" | "select" | "show" | "close" }>) {
  switch (intent.do) {
    case "show":
      return `Fast opened the canvas on the scratch pad ${intent.address}.`;
    case "close":
      return "Fast closed the canvas.";
    case "go":
      return `Fast opened ${intent.address}.`;
    case "scroll":
      return `Fast scrolled ${intent.where === "top" || intent.where === "bottom" ? `to the ${intent.where}` : intent.where}.`;
    case "panel":
      return `Fast ${intent.open ? "opened" : "closed"} the side panel.`;
    case "point":
      return `Fast pointed at ${JSON.stringify(intent.quote)}.`;
    case "select":
      return `Fast marked ${JSON.stringify(intent.quote)}.`;
  }
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
  /** The page whose text is in it, as address and text. */
  page: string | null;
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
  /** Whether a voice session is on: Fast runs only then, because only then can the user hear it. */
  private listening = false;

  constructor(private readonly provider: Provider, model: string, private readonly deps: FastDeps) {
    this.model = model;
    this.context = this.build(null);
    deps.log("context_built", { text: this.context.input[0].content, why: "start", cursor: this.context.cursor });
  }

  /** A voice session started or ended. When one starts, Fast gets what happened while none was on. */
  listen(on: boolean) {
    this.listening = on;
    if (!on) return;
    void this.warm(this.context, "voice on");
    this.wake("voice on");
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
    return this.handovers.length > 0 || this.deps.world.since(this.context.cursor).some((entry) => entry.kind === "update" || entry.kind === "problem");
  }

  /** One wake-up: the new entries in, one script out, its calls carried out. */
  private async run() {
    const run = ++this.runs;
    const began = Date.now();
    const context = this.context;
    const handovers = this.handovers.splice(0);
    const reasons = this.reasons.splice(0);
    const entries = this.deps.world.since(context.cursor);
    const page = await this.deps.page();
    const said = handovers.flatMap((handover) => handover.messages);
    const text = this.render(context, run, entries, said, page);
    if (entries.length > 0) context.cursor = entries.at(-1)!.n;
    context.input.push({ role: "user", content: text });
    this.deps.log("wake", { run, text, why: reasons, handovers: handovers.map((handover) => handover.id), entries: entries.map((entry) => entry.n), page: page?.address ?? null, page_ms: Date.now() - began });

    const carried: Carried = { run, handover: handovers.at(-1)?.id ?? null, said, used: [] };
    const scope: Scope = {
      messages: [...context.messages.values()].map(({ id, text, page, marked }) => ({ id, text, page, marked })),
      updates: [...context.updates.values()].map(({ id, text }) => ({ id, text })),
      agents: [...context.agents].filter((id) => this.deps.agent(id)),
      page: page && { address: page.address, text: page.text },
      pages: this.deps.pages().map(({ address }) => address),
      pads: page?.pads?.map(({ address }) => address) ?? [],
      handover: said.map((message) => message.id),
    };
    const written = await script(
      context.input,
      scope,
      (attempt) => this.call(context, { run, attempt, what: "script" }, { tool_choice: { type: "function", name: "exec" } }),
      (kind, fields) => this.deps.log(kind, { run, ...fields }),
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
  private render(context: Context, run: number, entries: Entry[], said: Message[], page: Seen | null) {
    for (const entry of entries) this.hold(context, entry);
    const before = entries.filter((entry) => entry.n <= context.handled);
    const fresh = entries.filter((entry) => entry.n > context.handled);
    context.handled = 0;
    const shown = page && `${page.address}\n${page.text}`;
    const unseen = shown !== null && shown !== context.page;
    if (unseen) context.page = shown;
    const world = this.deps.world;
    const known = [...world.recent("task", 200), ...world.recent("update", 400)].filter((entry) => this.deps.agent(entry.agent)).sort((a, b) => a.n - b.n);
    return wakeText(run, before, fresh, said, page, unseen, roster(known));
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
    const context: Context = { instructions: "", input: [], cursor: world.now, messages: new Map(), updates: new Map(), agents: new Set(), page: null, touched: 0, tokens: 0, handled: 0 };
    const pages = this.deps.pages();
    context.instructions = instructions(pages, this.deps.routes?.() ?? "");
    const lines = ["This is the start of your context. The session may have been running before it."];
    if (summary) lines.push("", "A summary of the conversation before:", summary);
    const messages = world.recent("message", RECENT_MESSAGES);
    if (messages.length > 0) lines.push("", "The most recent messages:", ...messages.map(line));
    const tasks = this.deps.tasks().slice(-RECENT_TASKS);
    const given = new Map(world.recent("task", 200).map((entry) => [entry.id, entry]));
    const shown: string[] = [];
    for (const task of tasks) {
      const entry = given.get(task.id);
      // A task from before the agents, or one whose agent is gone, cannot be followed up: it is left out.
      if (!entry || !this.deps.agent(entry.agent)) continue;
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
    case "say":
      return "Saying a line";
    case "spawn":
      return "Starting a new agent";
    case "send":
      return `Giving a${intent.agent} more work`;
    case "fork":
      return `Forking a${intent.agent}`;
    case "go":
      return `Opening ${intent.address}`;
    case "scroll":
      return "Scrolling";
    case "panel":
      return `${intent.open ? "Opening" : "Closing"} the side panel`;
    case "point":
      return `Pointing at "${intent.quote}"`;
    case "select":
      return `Marking "${intent.quote}"`;
    case "restart":
      return "Restarting";
  }
}
