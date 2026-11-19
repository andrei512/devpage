// The slow agents: each one is a conversation with an agent harness, with its own context. Which harness is not known
// here: it is behind `Harness`, and Claude Code is the one there is. Fast gives an agent a piece of work as one prompt;
// the agent does it in one turn and publishes updates on that task, the last of which says it is finished or failed.
// An agent runs one task at a time, and a follow-up waits its turn. A new agent starts empty and reads what its
// task needs, or is a fork of another agent, which then starts with what that one knows; where the harness cannot
// fork, a fork starts empty too. Between tasks an agent is only a saved conversation: it is picked up again
// when work arrives for it.
//
// Two things are kept apart. A task is the live session's: the work the user asked for, with its updates. A turn is the
// harness's: one message in, and what the agent did until it stopped. The harness keeps it, and it is read from there: nothing of a session is kept here. Today a task is
// done in one turn, and nothing in a session points at a task.
import { Registry, type Registering } from "./registry.ts";
import { SessionStore } from "./store.ts";
import { z } from "zod";
import { SHARED_BRIEF } from "./brief.ts";
import type { Harness, HarnessTool, Item, Turn, TurnEnd } from "./harness.ts";
import type { Part } from "./tasks.ts";

/** One piece of work for an agent. */
export interface Job {
  task: number;
  prompt: string;
}

export interface Published {
  agent: number;
  task: number;
  type: "progress" | "question" | "finished" | "failed";
  text: string;
  /** A few words naming the task, with the agent's first update. */
  title?: string;
  /** A few words naming the agent by what it works on: with its first update, and again when its scope changed. */
  name?: string;
  /** The places to look at, with the update that ends a task. */
  changes: Part[];
}

/** One message to an agent, and all it did until it stopped. */
export interface SessionTurn {
  /** Its place in the agent's session, from 1. */
  n: number;
  state: "running" | TurnEnd;
  started: number;
  /** The first is the message that started it. */
  items: Item[];
}

/** What happens in a session as it happens: a turn started with this message, an item of the turn that runs is new or changed, or that turn ended. */
export type SessionLine = { started: number; message: Item } | { item: Item } | { ended: number; state: TurnEnd };

/** The message that starts a turn says which task it is, on a line of its own, before the prompt. */
const TASK = /^Task (\d+)\n/;

/** A prompt as a chat shows it: without the line that names its task, and without what was added about the user's page. */
function shown(message: string) {
  return message.replace(TASK, "").split("\n\nThe page the user is on:")[0].trim();
}

export interface AgentHooks {
  atomic?: (write: () => void) => void;
  worldTools?: (agent: number | null) => HarnessTool[];
  /** An agent published an update on a task. */
  published: (update: Published) => void;
  /** Whether a task is this agent's and still running. */
  holds: (agent: number, task: number) => boolean;
  /** Saves a comment on the part that was marked when the task was given. Throws when there was none. */
  comment: (task: number, text: string) => Promise<void>;
  /** The session's log, for an agent that is asked who said what. */
  readLog: (filter: any) => unknown;
  /** Whether a task was given to this agent, running or over. */
  owns: (agent: number, task: number) => boolean;
  /** Something happened in an agent's session, now: for a page that has its chat open. */
  session: (agent: number, line: SessionLine) => void;
  /** An agent appeared, was named, started or stopped working, or was closed: the open pages are told. */
  changed: () => void;
  /** Something for the session's log. */
  log: (kind: string, fields: { agent?: number; task?: number; text?: string; [more: string]: unknown }) => void;
}

interface Agent {
  /** What it is, for good: its tasks and its orb hang on this number, not on its name. */
  id: number;
  /** What it works on, in a few words, as it named itself. None until its first update. */
  name: string | null;
  /** The harness it runs on, as the settings name it. It never changes: a conversation of one harness means nothing to another. */
  harness: string;
  /** The model it runs on. None: its harness's own default. */
  model: string | null;
  /** Its own conversation in the harness, as the harness names it, once it has run. */
  session: string | null;
  /** The conversation it starts as a copy of, until it has its own. */
  of: string | null;
  /** Where it came from: nothing, or the agent it is a fork of. */
  from: "fresh" | number;
  queue: Job[];
  /** The task it is doing now. */
  doing: number | null;
  /** Its last turn ended with a question for the user, and no work has come for it since. */
  asked: boolean;
  running: Turn | null;
  report?: "asked" | "ended";
}

interface Saved {
  next: number;
  /** `harness` and `model` are left out by a file from before there were routes: such an agent is of the first harness there was, Claude Code. */
  /** `from` is "base" in a file from when new agents were copies of an agent that had read the project. */
  agents: (Pick<Agent, "id" | "session" | "of"> & { from: Agent["from"] | "base"; name?: string | null; harness?: string; model?: string | null })[];
}

/** Where an agent runs: a harness, as the settings name it, and one of its models. */
export interface On {
  harness: string;
  model: string | null;
}

/** The harness of the agents from before there were several. */
const FIRST = "claude-code";

/** What every slow agent is told about its role. `pads` is the folder the scratch pads are in. */
const brief = (pads: string, fullFilesystemAccess: boolean) => `${SHARED_BRIEF}

You are one of the slow agents: you do the work. Fast gives you a piece of work as a message "Task <number>", with what the user said and, when it matters, the page the user was on and the part of it the user had marked: words such as "this" and "here" mean that part. A page's address is its file's path from the project root. Several agents work in this project at the same time: change only what your task needs.

Decide from the user's words what the task is. A question: find the answer, in the code, the files or another source, and change nothing. A command to change, write, fix, research or build something: do it, without asking the user to confirm. When the user was only thinking aloud or asked what you think, change nothing and give your opinion.

Nobody reads your replies. Only the \`update\` tool reaches the user, through Fast, which decides what the user hears. Publish on the task you were given:
- your first update names the task: set \`title\` to three to six words. Publish it at once, as a \`progress\` update that says what you are starting, before you do anything else;
- you have a name too, shown beside your tasks: two to four words for what you work on, wider than one task ("Sign-in page", not "Fix the reset link"). Set \`name\` with your very first update. Set it again only when a later task has changed what you work on;
- \`progress\`, only when there is something worth knowing while the user waits;
- \`question\`, only when you truly cannot go on without the user: exactly one question, then end your turn. The user's answer arrives as a new task: do the work then, and finish both tasks;
- \`finished\`, once: the answer, or what you changed and where. Its \`changes\` are the places the user should look at: for each, the file's path from the project root, or a scratch pad's address, and a short quote, a few words copied exactly from the new text as a reader sees it, not from its markup;
- \`failed\`, once: what went wrong.
Every task ends with \`finished\` or \`failed\`, and then you end your turn.

An update is spoken to the user as it is written. So write it to be heard: one to four plain sentences, in the first person, the answer or the result first, then what you did not verify, then what the user has to decide. No Markdown, no lists, no file paths read out unless the user needs them, no praise, no account of how you worked. Never ask a question back when the user wanted an answer: say what you assumed.

How to use the scratch pad. A scratch pad is a page that is not in the project: a Markdown file in the session's own folder, shown to the user on a canvas over their page while you two work out an idea. When a task names a scratch pad, or the page the user is on is one, the answer is drawn there: write that file, and nothing in the project unless the user asks for it. A pad's address is \`pads/\` and its file's name, and its file is in \`${pads}\`: the pad \`pads/how-it-works.md\` is the file \`${pads}/how-it-works.md\`. ${fullFilesystemAccess ? "This session allows reading and writing outside the project when the task requires it." : "Limit file reads and writes to the project and its scratch pads."} The user looks at a pad while talking, so make it something to look at and point at, not prose and not a document: a Mermaid diagram for parts and how they connect, a table for options compared on several points, a numbered list for a plan, exact names, numbers and wording where they matter. Keep it high level and small enough to take in at a glance: as a rule one picture, three or four short lines, at most one small table, and the two or three things the user has to decide. No sections of explanation, no reasons, no build steps, no list of limits: those you say, or the user asks for.

A pad is drawn in the open, part by part, and never in one save. The user watches the canvas from the moment the task is given: an empty canvas, and then a whole pad appearing at once, is the failure to avoid, even when the pad is small and you already know all of it. The order is fixed, for every new pad:
1. Your first update.
2. At once, your first save, before you read a file, search or think the idea through: \`# \` and the idea's title, and under it one placeholder for each part you plan, in the order they will stand. Nothing else. You do not need to know the content yet, only its shape: a picture, a few lines, a table, the things to decide.
3. Then the work: read what you need, and put the real part in the place of each placeholder, one at a time from the top, with one save for each part. Never two parts in one save.
4. A finished pad has no placeholder left. Then your \`finished\` update.
The plan may change while you work: add, drop or change a placeholder. A follow-up changes the pad that is there and keeps what the user did not ask to change; a part that is new or drawn again gets its placeholder first, and only a small correction is saved directly.

A placeholder is the \`Soon\` widget, alone on its line with an empty line above and below, and it is the only way to show on a pad that something is coming: never the word "Soon", an empty heading or a line of text in its place. It has no text: its shape says what is coming, and it moves while the user waits. A pad is drawn as any page is, so the site's widgets and diagrams work, and the canvas shows every save. The kinds: \`<Soon kind="flow" />\` for a diagram, \`<Soon kind="cards" count={2} />\` for options side by side, \`<Soon kind="list" />\`, \`<Soon kind="table" columns={3} />\`, \`<Soon kind="picture" />\` for a design or an image, and \`<Soon kind="text" />\`. Give each about the size of what will take its place, so the pad does not jump: \`height\` is \`"s"\`, \`"m"\`, \`"l"\`, \`"xl"\` or a number of pixels, and \`width\` is \`"quarter"\`, \`"third"\`, \`"half"\`, \`"two-thirds"\` or \`"three-quarters"\`, full when left out.

While a pad is open, every answer about its idea goes onto the pad first: when the user adds something, corrects you or asks what you think of a change, bring the pad up to date with it, and never answer at length while the pad stays as it was. "Design only" and "no changes" are about the project, not about the pad. Your update is then one or two sentences on what is on the pad and what the user should decide, never the pad read aloud, and its \`changes\` name the pad.

A comment is written only when the user asks for one ("comment this", "leave a note here that..."): then call \`write_comment\`, with a short text in the user's words. Nothing else is ever saved as a comment.

A question about who said something, or what exactly was said or done in this session: call \`read_log\` and answer only from its entries.`;

/** Added to a task that names a scratch pad, so the order of drawing is in front of the agent when it starts, whatever its conversation holds. */
const PAD_ORDER = `When this task has you draw a scratch pad or a new part of one, the order is not optional: right after your first update and before you read anything, save the pad as its title and one \`Soon\` widget for each part you plan, written as a tag such as \`<Soon kind="list" height="m" />\`, alone on its line. These widgets are the only way progress is shown on a pad: never the word "Soon", a heading for a part that is not drawn yet, or any other text that says something is coming. Then put the real part in the place of each, one at a time from the top, with one save for each. Never write a pad in one save.`;

/** A task about a scratch pad: Fast names the pad's address, or it is the page the user is on. */
const aboutPad = (prompt: string) => /(^|[^\w/.-])pads\/[a-z0-9][a-z0-9-]*\.md\b/.test(prompt);

export class Agents {
  private readonly tools: (agent: number | null) => HarnessTool[];
  private readonly agents = new Map<number, Agent>();
  private next = 1;

  /** `root` is the project's folder and `pads` the folder of the scratch pads. `harnesses` are the ones that are enabled, by the name the settings give each. `routes` are the enabled routes, in
      their order: the first is the one an agent runs on when nothing else is said. */
  constructor(private readonly root: string, private readonly pads: string, private readonly harnesses: Map<string, Harness>, private readonly routes: On[], private readonly store: SessionStore, private readonly hooks: AgentHooks, first = 1, private readonly fullFilesystemAccess = false) {
    // A number is never given twice: tasks are kept by their agent's number.
    this.next = first;
    if (hooks.worldTools) {
      this.tools = hooks.worldTools;
    } else {
      // A standalone harness uses the same registered functions as the service.
      const registry = new Registry(store.db);
      registry.part("work", part => registerAgentFunctions(part, () => this));
      this.tools = agent => registry.tools(agent);
    }
    const saved = store.get<Saved>("agents");
    if (saved) {
      this.next = Math.max(first, saved.next);
      for (const agent of saved.agents) {
        const harness = agent.harness ?? FIRST;
        // An agent of a harness that is no longer enabled cannot work: it is closed.
        if (!harnesses.has(harness)) continue;
        // Such an agent that never ran has only the copy to start from, which is no longer kept up: it starts empty.
        const based = agent.from === "base";
        this.agents.set(agent.id, { ...agent, from: agent.from === "base" ? "fresh" : agent.from, of: based ? null : agent.of, harness, model: agent.model ?? null, name: agent.name ?? null, queue: [], doing: null, asked: false, running: null });
      }
    }
  }

  /** The harness an agent runs on. */
  private of(agent: Agent) {
    return this.harnesses.get(agent.harness)!;
  }

  private save() {
    const saved: Saved = { next: this.next, agents: [...this.agents.values()].map(({ id, name, harness, model, session, of, from }) => ({ id, name, harness, model, session, of, from })) };
    this.store.set("agents", saved);
  }

  /** The agents that exist, with what each is doing. A closed agent is not among them. */
  all() {
    return [...this.agents.values()].map(({ id, name, harness, model, from, doing, asked, queue }) => ({
      id,
      name,
      harness,
      model,
      from,
      doing,
      waiting: queue.map((job) => job.task),
      state: doing !== null || queue.length > 0 ? ("working" as const) : asked ? ("asked" as const) : ("idle" as const),
    }));
  }

  has(id: number) {
    return this.agents.has(id);
  }

  /** What happened inside an agent: its turns, oldest first, each with its items. Also for an agent that was closed. */
  async session(id: number): Promise<SessionTurn[]> {
    const agent = this.agents.get(id);
    // A closed agent's conversation is still with the harness, but which one it was is forgotten with the agent.
    const history = agent && this.of(agent).history;
    if (!agent?.session || !history) return [];
    const past = await history.call(this.of(agent), agent.session, { root: this.root, tools: this.tools(null) });
    // A fork's conversation starts with its parent's: its own begins at the first task that was given to it.
    const own = past.findIndex((turn) => {
      const task = TASK.exec(turn.items[0]?.line ?? "");
      return task !== null && this.hooks.owns(id, Number(task[1]));
    });
    if (own < 0) return [];
    const turns = past.slice(own);
    return turns.map((turn, index) => {
      const running = index === turns.length - 1 && agent.doing !== null;
      return {
        n: index + 1,
        state: running ? ("running" as const) : ("done" as const),
        started: turn.started,
        // A call left without an answer in a turn that is over did end, with the turn.
        items: turn.items.map((item, at) => (at === 0 ? { ...item, line: shown(item.line) } : !running && item.state === "running" ? { ...item, state: "done" as const } : item)),
      };
    });
  }

  /** A new agent for a piece of work, on a route, or on the default one. It starts empty. */
  spawn(job: Job, on: On = this.routes[0]) {
    if (!this.harnesses.has(on.harness)) throw new Error(`no harness ${on.harness} is enabled`);
    return this.create("fresh", null, job, on);
  }

  /** A new agent that starts with what another one knows, on that one's route. Where the harness cannot fork, it starts empty. */
  fork(id: number, job: Job) {
    const parent = this.agents.get(id);
    if (!parent) throw new Error(`no agent a${id} exists`);
    // A parent that has not run yet has nothing of its own: its fork starts where it starts.
    return this.create(id, this.of(parent).can.fork ? (parent.session ?? parent.of) : null, job, parent);
  }

  /** More work for an agent that exists. It waits while the agent is busy. */
  send(id: number, job: Job) {
    const agent = this.agents.get(id);
    if (!agent) throw new Error(`no agent a${id} exists`);
    this.give(agent, job);
    return agent.id;
  }

  /** Closes the agents nobody will give work to any more, except those in `keep` and those that are working. Their work is on disk. */
  forget(keep: Set<number>) {
    const closed: number[] = [];
    for (const agent of this.agents.values()) {
      if (keep.has(agent.id) || agent.doing !== null || agent.queue.length > 0) continue;
      this.agents.delete(agent.id);
      closed.push(agent.id);
    }
    if (closed.length > 0) {
      this.save();
      this.hooks.log("agents_closed", { text: closed.map((id) => `a${id}`).join(", "), kept: [...this.agents.keys()] });
      this.hooks.changed();
    }
    return closed;
  }

  private create(from: Agent["from"], of: string | null, job: Job, on: On) {
    const agent: Agent = { id: this.next++, name: null, harness: on.harness, model: on.model, session: null, of, from, queue: [], doing: null, asked: false, running: null };
    this.agents.set(agent.id, agent);
    this.save();
    this.hooks.log("agent_created", { agent: agent.id, task: job.task, harness: agent.harness, model: agent.model, from: typeof from === "number" ? `a${from}` : from, forked_session: of });
    this.give(agent, job);
    return agent.id;
  }

  private give(agent: Agent, job: Job) {
    agent.queue.push(job);
    this.hooks.log("task_to_agent", { agent: agent.id, task: job.task, text: job.prompt, waits_behind: agent.doing });
    if (agent.doing === null) void this.work(agent);
  }

  /** Does the agent's waiting tasks, one after the other. */
  private async work(agent: Agent) {
    for (let job = agent.queue.shift(); job; job = agent.queue.shift()) {
      agent.doing = job.task;
      agent.asked = false;
      this.hooks.changed();
      try {
        await this.turn(agent, job);
      } catch (error) {
        const why = error instanceof Error ? error.message : String(error);
        this.hooks.log("agent_error", { agent: agent.id, task: job.task, text: why });
        if (this.hooks.holds(agent.id, job.task)) this.hooks.published({ agent: agent.id, task: job.task, type: "failed", text: `I could not do it: ${why}`, changes: [] });
      } finally {
        agent.doing = null;
        agent.running = null;
        this.hooks.changed();
      }
    }
  }

  /** One task, as one turn of the agent: from its prompt until the agent stops. */
  private async turn(agent: Agent, job: Job, again = false): Promise<void> {
    const began = Date.now();
    /** How the task stands after the agent's own updates in this turn. */
    delete agent.report;
    const forking = !agent.session && Boolean(agent.of);
    const tell = (line: SessionLine) => this.hooks.session(agent.id, line);
    tell({ started: began, message: { id: `sent-${began}`, kind: "user_message", state: "done", at: began, line: shown(job.prompt) } });
    const running = this.of(agent).turn({
      model: agent.model,
      session: agent.session ?? agent.of,
      fork: forking,
      message: `Task ${job.task}\n${job.prompt}${aboutPad(job.prompt) ? `\n\n${PAD_ORDER}` : ""}`,
      ...this.workspace(),
      tools: this.tools(agent.id),
    });
    agent.running = running;
    let end: TurnEnd | null = null;
    try {
      for await (const event of running.events) {
        if (event.type === "started") {
          if (agent.session !== event.session) {
            agent.session = event.session;
            this.save();
          }
          this.hooks.log("agent_turn_started", { agent: agent.id, task: job.task, harness: agent.harness, harness_session: event.session, forked: forking, model: event.model ?? agent.model, ms: Date.now() - began });
        } else if (event.type === "item") {
          tell({ item: event.item });
          // What it thought aloud and which tools it used: nobody hears this, it is here to debug with.
          if (event.item.kind === "note") this.hooks.log("agent_text", { level: "debug", agent: agent.id, task: job.task, text: event.item.line });
          else if (event.item.state === "running") this.hooks.log("agent_tool", { level: "debug", agent: agent.id, task: job.task, name: event.item.tool, text: event.item.line, input: event.item.detail });
        } else {
          end = event.state;
          this.hooks.log("agent_turn_ended", { agent: agent.id, task: job.task, text: event.text, end: event.state, state: agent.report ?? "open", ms: Date.now() - began, ...event.detail });
          // A turn that ended without saying how the task ended: its last words are the task's end.
          if (!agent.report && this.hooks.holds(agent.id, job.task)) {
            const failed = event.state !== "done";
            this.hooks.log("agent_ended_without_update", { agent: agent.id, task: job.task });
            this.hooks.published({ agent: agent.id, task: job.task, type: failed ? "failed" : "finished", text: event.text.trim() || (failed ? "It stopped without saying why." : "Done."), changes: [] });
          }
        }
      }
    } catch (error) {
      const why = error instanceof Error ? error.message : String(error);
      // A conversation that cannot be copied or picked up any more: the agent starts empty, once.
      if (!again && !agent.report && (agent.session || agent.of)) {
        this.hooks.log("agent_session_lost", { agent: agent.id, task: job.task, text: why, harness_session: agent.session, of: agent.of });
        tell({ ended: Date.now(), state: "failed" });
        agent.session = null;
        agent.of = null;
        this.save();
        return this.turn(agent, job, true);
      }
      tell({ item: { id: `lost-${Date.now()}`, kind: "notice", state: "failed", at: Date.now(), line: why } });
      tell({ ended: Date.now(), state: "failed" });
      throw error;
    }
    // A harness that stopped giving events without saying how the turn ended: taken as failed.
    tell({ ended: Date.now(), state: end ?? "failed" });
  }

  /** Register the work part before creating agents, so every agent has the full tool list. */
  static registerFunctions(as: Registering, agents: () => Agents) {
    as.set("update", {
      for: "slow",
      tool: { name: "update", always: true, reads: ({ type, text }) => ({ kind: type === "question" ? "question" : "agent_message", line: String(text ?? ""), ...(type === "failed" ? { state: "failed" as const } : {}) }) },
      description: "Publishes an update on a task: it is what the user hears, through Fast. Every task ends with one `finished` or one `failed`.",
      input: {
        task: z.number().describe("The task's number"),
        type: z.enum(["progress", "question", "finished", "failed"]),
        text: z.string().describe("One to four plain spoken sentences in the first person, no Markdown: the answer or the result first"),
        title: z.string().optional().describe("Three to six words naming the task: with your first update on it"),
        name: z.string().optional().describe("Two to four words naming you by what you work on, across your tasks: with your very first update, and again only when your scope has changed"),
        changes: z.array(z.object({ page: z.string(), quote: z.string() })).optional().describe("With `finished`: the places to look at"),
      },
    }, ({ task, type, text, title, name, changes }, { caller }) => {
      const live = agents();
      const agent = caller.who === "slow" && caller.agent !== null ? live.agents.get(caller.agent) : null;
      if (!agent || !live.hooks.holds(agent.id, task)) throw new Error(`Task ${task} is not a running task of yours.`);
      const publish = () => {
        const named = name?.trim();
        if (named && named !== agent.name) {
          live.hooks.log("agent_named", { agent: agent.id, task, text: named, was: agent.name });
          agent.name = named;
          live.save();
          live.hooks.changed();
        }
        if (type === "question") {
          agent.report = "asked";
          agent.asked = true;
        }
        if (type === "finished" || type === "failed") agent.report = "ended";
        live.hooks.published({ agent: agent.id, task, type, text, title, ...(named ? { name: named } : {}), changes: changes ?? [] });
      };
      if (live.hooks.atomic) live.hooks.atomic(publish);
      else publish();
      return "Published.";
    });
    as.action("write_comment", {
      for: "slow",
      tool: { name: "write_comment", always: true },
      description: "Saves a comment on the part that was marked when the task was given. Only when the user explicitly asked to leave a comment or a note there.",
      input: { task: z.number().describe("The task's number"), text: z.string().describe("The note, short and in the user's words") },
    }, async ({ task, text }, { caller }) => {
      const live = agents();
      if (caller.who !== "slow" || caller.agent === null || !live.hooks.owns(caller.agent, task)) throw new Error(`Task ${task} is not yours.`);
      await live.hooks.comment(task, text);
      return "Saved.";
    });
    as.get("read_log", {
      for: "slow",
      tool: { name: "read_log" },
      description:
        "Reads the session's log of who said and did what, exactly: the most recent entries, oldest first. Changes nothing. `who` is `user` (what the user said, as it was heard), `voice` (what was spoken aloud), `fast` (Fast's runs and scripts), `say` (what Fast gave the voice to say), `agent` (the slow agents: tasks, updates, tools), `world` (every entry of the world) or `service`.",
      input: {
        limit: z.number().optional().describe("How many entries at most; 50 when left out, 500 at most"),
        since: z.string().optional().describe("Only entries at or after this ISO time"),
        who: z.enum(["user", "voice", "fast", "say", "agent", "world", "service"]).optional(),
        text: z.string().optional().describe("Only entries whose text has these words in it"),
      },
    }, filter => agents().hooks.readLog(filter));
  }

  private workspace() {
    return {
      root: this.root,
      also: [this.pads],
      fullFilesystemAccess: this.fullFilesystemAccess,
      brief: brief(this.pads, this.fullFilesystemAccess),
    };
  }

}

export const registerAgentFunctions = Agents.registerFunctions;
