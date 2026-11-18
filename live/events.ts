// The numbered event stream shared by registered parts and the built-in world.
import type { Database } from "bun:sqlite";
import type { Entry } from "./registry.ts";
import type { Entry as CoreEntry } from "./world.ts";

const coreParts = { message: "session", problem: "session", page: "page", task: "work", update: "work" } as const;

/** The built-in view used by the transcript, tasks and sandbox variables. */
export function coreEntry(entry: Entry): CoreEntry | null {
  if (coreParts[entry.kind as keyof typeof coreParts] !== entry.part) return null;
  return { ...entry.fields, n: entry.n, at: entry.at, kind: entry.kind } as CoreEntry;
}

export class Events {
  constructor(private readonly db: Database) {}

  install() {
    this.db.run("create table if not exists entries (n integer primary key autoincrement, at text not null, part text not null, kind text not null, fields text not null)");
    // Two historical streams could each have an entry numbered one. Keep the
    // former extension number when joining them requires moving its cursor.
    this.db.run("create table if not exists original_entry_numbers (n integer primary key, original integer not null)");
  }

  get now() {
    return Number(this.db.query("select coalesce(max(n), 0) from entries").values()[0][0]);
  }

  since(after: number): Entry[] {
    const rows = this.db.query<Omit<Entry, "fields"> & { fields: string; original: number | null }, [number]>(
      "select e.*, o.original from entries e left join original_entry_numbers o on o.n=e.n where e.n > ? order by e.n",
    ).all(after);
    return rows.map(({ fields, original, ...row }) => ({
      ...row, fields: JSON.parse(fields), ...(original === null ? {} : { originalNumber: original }),
    }));
  }

  append(part: string, kind: string, fields: Record<string, unknown>, at = new Date().toISOString(), number?: number): Entry {
    const result = this.db.run("insert into entries (n, at, part, kind, fields) values (?, ?, ?, ?, ?)", [number ?? null, at, part, kind, JSON.stringify(fields)]);
    return { n: Number(result.lastInsertRowid), at, part, kind, fields };
  }

  appendCore(entry: Omit<CoreEntry, "n">, number?: number) {
    const { kind, at, ...fields } = entry;
    return this.append(coreParts[kind], kind, fields, at, number).n;
  }

  core(): CoreEntry[] {
    return this.since(0).map(coreEntry).filter(entry => entry !== null);
  }

  /** Called inside the import transaction. Core numbers are referenced by old
   * logs. Move an overlapping extension suffix as a block to retain its order. */
  importCore(history: CoreEntry[]) {
    if (!history.length) return;
    const numbers = new Set<number>();
    let cursor = this.now;
    for (const entry of history) {
      if (!Number.isSafeInteger(entry.n) || entry.n < 1) throw new Error("Invalid world entry number");
      numbers.add(entry.n);
      cursor = Math.max(cursor, entry.n);
    }
    const firstCollision = this.since(0).find(e => numbers.has(e.n));
    if (firstCollision) {
      const suffix = this.since(firstCollision.n - 1);
      for (const entry of suffix) {
        this.db.run("update entries set n = ? where n = ?", [++cursor, entry.n]);
        this.db.run("insert into original_entry_numbers values (?, ?)", [cursor, entry.n]);
      }
    }
    for (const { n, ...entry } of history) this.appendCore(entry, n);
  }
}
