// The world, as one numbered log: every message, every change of the user's page, every task given to an agent and every
// update an agent published is one entry with a number and a time. "Everything since entry 412" is then exact: it is
// what Fast gets when it is woken, and what its successor gets at a swap. The log is a file, so the world is still
// there after a restart.
import { appendFileSync, existsSync, mkdirSync, readFileSync } from "node:fs";
import { dirname } from "node:path";

interface Base {
  /** The entry's number: one more than the entry before it. */
  n: number;
  /** When it happened, ISO. */
  at: string;
}

/** Something the user said, or Voice said. One hand-over's fragments are one message. */
export interface Message extends Base {
  kind: "message";
  id: number;
  who: "me" | "voice";
  text: string;
  /** The page the user was on when it was said. */
  page: string | null;
  /** The text the user had marked when it was said; empty when nothing was marked. */
  marked: string;
  /** Where it is on the voice session's own clock. */
  start_ms: number | null;
  end_ms: number | null;
  /** The hand-over it came with, when Voice handed it over. */
  handover: string | null;
  /** For a line of Voice's that passes on what agents reported: those agents. */
  about?: number[];
}

/** Something that happened on the user's page: the user opened one, marked a part, or Fast moved it. */
export interface PageChange extends Base {
  kind: "page";
  by: "me" | "fast";
  /** What happened, as a sentence. */
  text: string;
}

/** Work Fast gave to an agent. */
export interface TaskGiven extends Base {
  kind: "task";
  id: number;
  agent: number;
  /** How the agent was picked: a new one, an existing one, or a fork of one. */
  how: "spawn" | "send" | "fork";
  /** The agent it was forked from, for a fork. */
  from: number | null;
  prompt: string;
}

/** What an agent published on a task. */
export interface Update extends Base {
  kind: "update";
  id: number;
  task: number;
  agent: number;
  type: "progress" | "question" | "finished" | "failed";
  text: string;
}

/** Something a script asked for that could not be done. */
export interface Problem extends Base {
  kind: "problem";
  text: string;
}

export type Entry = Message | PageChange | TaskGiven | Update | Problem;
/** An entry before it has its number and time. */
type Fresh<T extends Entry = Entry> = T extends Entry ? Omit<T, "n" | "at"> : never;

/** How many entries are kept in memory. Older ones stay in the file. */
const KEPT = 2000;

export class World {
  private entries: Entry[] = [];
  private last = 0;
  private ids = { message: 0, update: 0 };

  /** `added` is called with every new entry. */
  constructor(private readonly file: string, private readonly added: (entry: Entry) => void) {
    if (!existsSync(file)) return;
    for (const line of readFileSync(file, "utf8").split("\n")) {
      if (!line) continue;
      try {
        const entry = JSON.parse(line) as Entry;
        this.entries.push(entry);
        this.last = Math.max(this.last, entry.n);
        if (entry.kind === "message" || entry.kind === "update") this.ids[entry.kind] = Math.max(this.ids[entry.kind], entry.id);
      } catch {}
    }
    this.entries = this.entries.slice(-KEPT);
  }

  /** The number of the newest entry. */
  get now() {
    return this.last;
  }

  /** The next id of a message or an update: they go on across restarts, so `m41` is one message for ever. */
  next(kind: "message" | "update") {
    return (this.ids[kind] += 1);
  }

  add<T extends Entry>(entry: Fresh<T>): T {
    const full = { n: (this.last += 1), at: new Date().toISOString(), ...entry } as unknown as T;
    this.entries.push(full);
    if (this.entries.length > KEPT) this.entries.shift();
    try {
      mkdirSync(dirname(this.file), { recursive: true });
      appendFileSync(this.file, JSON.stringify(full) + "\n");
    } catch (error) {
      console.error("world:", error);
    }
    this.added(full);
    return full;
  }

  /** Every entry after the one with this number, oldest first. */
  since(n: number) {
    return this.entries.filter((entry) => entry.n > n);
  }

  /** The most recent entries of a kind, oldest first. */
  recent<K extends Entry["kind"]>(kind: K, most: number) {
    return this.entries.filter((entry): entry is Extract<Entry, { kind: K }> => entry.kind === kind).slice(-most);
  }

  /** The updates of a task, oldest first. */
  updates(task: number) {
    return this.entries.filter((entry): entry is Update => entry.kind === "update" && entry.task === task);
  }
}

/** A text on one line, in quotes, as Fast reads it. */
const quoted = (text: string) => JSON.stringify(text.replace(/\s+/g, " ").trim());

/** An entry as one line of Fast's context. The ids in it are the script's variables. */
export function line(entry: Entry): string {
  switch (entry.kind) {
    case "message":
      return `m${entry.id}  ${entry.who === "me" ? "user" : `voice${entry.about?.length ? ` (${entry.about.map((agent) => `a${agent}'s`).join(" and ")} answer)` : ""}`}  ${quoted(entry.text)}${entry.who === "me" && entry.page ? `  page ${entry.page}` : ""}${entry.marked ? `  marked ${quoted(entry.marked)}` : ""}`;
    case "page":
      return `page  ${entry.text}`;
    case "task":
      return `t${entry.id}  given to a${entry.agent} (${entry.how === "spawn" ? "a new agent" : entry.how === "fork" ? `a new agent, forked from a${entry.from}` : "the same agent"})  ${quoted(entry.prompt.length > 240 ? `${entry.prompt.slice(0, 240)}…` : entry.prompt)}`;
    case "update":
      return `u${entry.id}  t${entry.task} a${entry.agent}  ${entry.type}  ${quoted(entry.text)}`;
    case "problem":
      return `problem  ${entry.text}`;
  }
}
