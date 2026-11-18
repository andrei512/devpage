// The live session's log: who said what, exactly. One file per day, one JSON entry per line, only ever added to.
// It holds words, never audio and never a key.
import { appendFileSync, existsSync, mkdirSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { stateOf } from "./home.ts";

/**
 * Who an entry is from:
 * - `user`: the user, as Voice heard them: the transcript's fragments and the messages made of them.
 * - `voice`: Voice: what it spoke aloud, and its hand-overs.
 * - `fast`: Fast: each wake-up with what it was shown, its script, and what the script's calls did.
 * - `say`: a line Voice was given to say.
 * - `agent`: the slow agents: the tasks they got, what they did, how each turn ended.
 * - `world`: every entry of the world, as the line Fast reads.
 * - `service`: what the service did around them.
 *
 * Entries with `level: "debug"` are the detail: fragments, model calls, an agent's tool calls.
 * Older entries can have other values (`slow`, `worker`, `luna`, `claude`), from before the agents were split.
 */
export type Who = "user" | "voice" | "fast" | "say" | "agent" | "world" | "service";

export interface Entry {
  /** When the entry was written, ISO. */
  ts: string;
  /** The voice session it belongs to; null when none was on. */
  session: string | null;
  who: Who;
  /** What kind of entry it is: `utterance`, `handover`, `wake`, `script`, `did`, `line`, `task_to_agent`, ... */
  kind: string;
  /** The words themselves, exactly. */
  text?: string;
  [more: string]: unknown;
}

export interface LogFilter {
  /** At most this many entries, the most recent ones. */
  limit?: number;
  /** Only entries at or after this ISO time. */
  since?: string;
  who?: Who;
  /** Only entries whose text has these words in it, in any case. */
  text?: string;
}

/** The local day, as the file is named. */
function day(date: Date) {
  const two = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${two(date.getMonth() + 1)}-${two(date.getDate())}`;
}

/** A copy of an API event that is safe and small enough to log: no audio, no long strings. */
export function shape(value: unknown, depth = 0): unknown {
  if (typeof value === "string") return value.length > 300 ? `${value.slice(0, 300)}… (${value.length} characters)` : value;
  if (value === null || typeof value !== "object") return value;
  if (depth > 5) return "…";
  if (Array.isArray(value)) return value.slice(0, 20).map((item) => shape(item, depth + 1));
  const copy: Record<string, unknown> = {};
  for (const [name, inner] of Object.entries(value)) {
    if (/audio|authorization|token|secret|key$/i.test(name) && typeof inner === "string") copy[name] = `(${inner.length} characters, not logged)`;
    else copy[name] = shape(inner, depth + 1);
  }
  return copy;
}

export class Log {
  private readonly folder: string;
  /** The voice session that is on now. */
  session: string | null = null;

  constructor(folder: string) {
    this.folder = folder;
    mkdirSync(folder, { recursive: true });
  }

  /** Adds an entry. It never throws: a log that cannot be written must not stop the session. */
  write(who: Who, kind: string, fields: { text?: string; [more: string]: unknown } = {}) {
    const now = new Date();
    const entry: Entry = { ts: now.toISOString(), session: this.session, who, kind, ...fields };
    try {
      appendFileSync(join(this.folder, `${day(now)}.jsonl`), JSON.stringify(entry) + "\n");
    } catch (error) {
      console.error("log:", error);
    }
  }

  /** The most recent entries that pass the filter, oldest first. */
  read({ limit = 50, since, who, text }: LogFilter = {}): Entry[] {
    const most = Math.max(1, Math.min(500, Math.floor(limit)));
    const from = since ? new Date(since) : null;
    if (from && Number.isNaN(from.getTime())) throw new Error(`"${since}" is not a time: use an ISO timestamp`);
    const words = text?.toLowerCase();
    const files = existsSync(this.folder) ? readdirSync(this.folder).filter((name) => /^\d{4}-\d{2}-\d{2}\.jsonl$/.test(name)).sort().reverse() : [];
    const found: Entry[] = [];
    for (const file of files) {
      const lines = readFileSync(join(this.folder, file), "utf8").split("\n");
      let older = false;
      for (let at = lines.length - 1; at >= 0 && found.length < most; at -= 1) {
        if (!lines[at]) continue;
        let entry: Entry;
        try {
          entry = JSON.parse(lines[at]);
        } catch {
          continue;
        }
        if (from && new Date(entry.ts) < from) {
          older = true;
          break;
        }
        if (who && entry.who !== who) continue;
        if (words && !(typeof entry.text === "string" && entry.text.toLowerCase().includes(words))) continue;
        found.push(entry);
      }
      if (older || found.length >= most) break;
    }
    return found.reverse();
  }
}

// Run by hand in the project's folder, it prints the log so that it can be read: `bun log.ts [-v] [how many] [words]`. `-v` adds the detail.
if (import.meta.main) {
  const given = process.argv.slice(2);
  const detail = given.includes("-v");
  const rest = given.filter((value) => value !== "-v");
  const limit = Number(rest.find((value) => /^\d+$/.test(value)) ?? 80);
  const words = rest.find((value) => !/^\d+$/.test(value));
  const entries = new Log(join(stateOf(process.cwd()), "log")).read({ limit: 500, text: words }).filter((entry) => detail || entry.level !== "debug");
  for (const entry of entries.slice(-limit)) {
    const { ts, session: _, who, kind, text, level: __, ...more } = entry;
    const about = [more.run !== undefined && `run ${more.run}`, more.agent !== undefined && (typeof more.agent === "number" ? `a${more.agent}` : String(more.agent)), more.task !== undefined && `t${more.task}`, more.ms !== undefined && `${more.ms} ms`].filter(Boolean).join(" ");
    const time = new Date(ts).toLocaleTimeString("en-GB", { hour12: false }) + "." + ts.slice(20, 23);
    console.log(`${time}  ${who.padEnd(7)} ${kind}${about ? `  [${about}]` : ""}`);
    if (text) console.log(text.split("\n").map((part) => `    ${part}`).join("\n"));
    if (detail) {
      const { run: _r, agent: _a, task: _t, ms: _m, ...others } = more;
      if (Object.keys(others).length > 0) console.log(`    ${JSON.stringify(others)}`);
    }
  }
}
