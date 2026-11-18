// The world as registered parts: the prototype of the extendable world. A part registers its entry kinds, its
// functions and its state; a function is written once and says who it is for, and it is offered to Fast as a call in
// its script and to a slow agent as a tool. What a function writes, state and entries, is kept in one SQLite file and
// committed in one transaction. Nothing of the running session uses this yet.
import { Database } from "bun:sqlite";
import { readFileSync } from "node:fs";
import { QuickJS } from "quickjs-wasi";
import { z } from "zod";
import type { HarnessTool } from "./harness.ts";

/** Who may call a function. */
export type For = "fast" | "slow" | "both";

/** Who called: a run of Fast, with its hand-over and what its script read, or a slow agent. */
export type Caller = { who: "fast"; run: number; handover: string | null; used: string[] } | { who: "slow"; agent: number };

/** One thing that happened, as the store keeps it. */
export interface Entry {
  n: number;
  at: string;
  part: string;
  kind: string;
  fields: Record<string, unknown>;
}

/** A kind of entry: the line Fast reads, an image when it has one, and whether it wakes Fast. */
export interface Kind {
  line: (fields: any) => string;
  /** A data URL or an address of an image the entry carries. */
  image?: (fields: any) => string | null;
  wakes?: boolean | ((fields: any, by: Caller) => boolean);
}

/** What a function is run with: its caller, its part's state, and a way to add entries. Writes are kept until the function ended. */
export interface Running {
  caller: Caller;
  get: (key: string) => unknown;
  set: (key: string, value: unknown) => void;
  add: (kind: string, fields: Record<string, unknown>) => void;
}

export interface Fn {
  part: string;
  name: string;
  does: "get" | "set" | "action";
  for: For;
  description: string;
  /** Its line of Fast's brief, for a function Fast may call. */
  brief?: string;
  input: Record<string, z.ZodType>;
  run: (input: any, running: Running) => unknown | Promise<unknown>;
}

type Options = Pick<Fn, "for" | "description" | "brief" | "input">;

/** What a part registers with. An extension's module gets this, with the schema builder: it cannot import the program's packages itself. */
export interface Registering {
  z: typeof z;
  kind: (name: string, kind: Kind) => void;
  get: (name: string, options: Options, run: Fn["run"]) => void;
  set: (name: string, options: Options, run: Fn["run"]) => void;
  action: (name: string, options: Options, run: Fn["run"]) => void;
  /** A widget the part brings, and its place. Recorded only: placing is devpage's. */
  widget: (name: string, place: "navbar" | "panel" | "float") => void;
}

/** One thing a script of Fast's asked for. */
export interface Asked {
  do: string;
  input: Record<string, unknown>;
}

let compiled: Promise<WebAssembly.Module> | null = null;
const binary = () => (compiled ??= WebAssembly.compile(readFileSync(new URL(import.meta.resolve("quickjs-wasi/quickjs.wasm")))));

export class Registry {
  private readonly db: Database;
  private readonly kinds = new Map<string, Kind & { part: string }>();
  private readonly fns = new Map<string, Fn>();
  readonly widgets: { part: string; name: string; place: string }[] = [];
  /** Called with each new entry and whether it wakes Fast. */
  onEntry: (entry: Entry, wakes: boolean) => void = () => {};
  /** Called with a part's whole state when it changed: what a page is sent. */
  onState: (part: string, state: Record<string, unknown>) => void = () => {};

  constructor(file: string) {
    this.db = new Database(file);
    this.db.run("create table if not exists entries (n integer primary key autoincrement, at text not null, part text not null, kind text not null, fields text not null)");
    this.db.run("create table if not exists state (part text not null, key text not null, value text not null, primary key (part, key))");
  }

  /** Registers a part: the core's or an extension's, the same way. */
  part(part: string, register: (as: Registering) => void) {
    const fn = (does: Fn["does"]) => (name: string, options: Options, run: Fn["run"]) => {
      if (does === "get" && options.for !== "slow") throw new Error(`${part}.${name}: a get is for slow agents only: Fast's script gets no answer from a call`);
      const full = `${part}.${name}`;
      if (this.fns.has(full)) throw new Error(`${full} is registered already`);
      this.fns.set(full, { part, name, does, run, ...options });
    };
    register({
      z,
      kind: (name, kind) => void this.kinds.set(`${part}.${name}`, { ...kind, part }),
      get: fn("get"),
      set: fn("set"),
      action: fn("action"),
      widget: (name, place) => void this.widgets.push({ part, name, place }),
    });
  }

  /** Loads an extension's world module: a file whose default export registers its part. */
  async load(part: string, file: string) {
    const module = await import(file);
    if (typeof module.default !== "function") throw new Error(`${file} has no default export that registers a part`);
    this.part(part, module.default);
  }

  /** A part's state, whole. */
  state(part: string): Record<string, unknown> {
    const rows = this.db.query("select key, value from state where part = ?").all(part) as { key: string; value: string }[];
    return Object.fromEntries(rows.map(({ key, value }) => [key, JSON.parse(value)]));
  }

  /** Every entry after the one with this number, oldest first. */
  since(n: number): Entry[] {
    const rows = this.db.query("select n, at, part, kind, fields from entries where n > ? order by n").all(n) as any[];
    return rows.map((row) => ({ ...row, fields: JSON.parse(row.fields) }));
  }

  /** An entry as the line Fast reads. */
  line(entry: Entry) {
    return this.kinds.get(`${entry.part}.${entry.kind}`)?.line(entry.fields) ?? `${entry.part}.${entry.kind}`;
  }

  /** The image an entry carries, when it has one. */
  image(entry: Entry) {
    return this.kinds.get(`${entry.part}.${entry.kind}`)?.image?.(entry.fields) ?? null;
  }

  /**
   * Calls a function for a caller. Its input is checked, and what it wrote, state and entries, is committed in one
   * transaction once it ended: a function that throws has written nothing.
   */
  async call(name: string, input: unknown, caller: Caller) {
    const fn = this.fns.get(name);
    if (!fn) throw new Error(`there is no function ${name}`);
    if (fn.for !== "both" && fn.for !== caller.who) throw new Error(`${name} is for ${fn.for}, not for ${caller.who}`);
    const given = z.object(fn.input).parse(input);
    const sets = new Map<string, unknown>();
    const adds: { kind: string; fields: Record<string, unknown> }[] = [];
    const result = await fn.run(given, {
      caller,
      get: (key) => (sets.has(key) ? sets.get(key) : this.state(fn.part)[key]),
      set: (key, value) => void sets.set(key, value),
      add: (kind, fields) => {
        if (!this.kinds.has(`${fn.part}.${kind}`)) throw new Error(`${fn.part} has no entry kind ${kind}`);
        adds.push({ kind, fields });
      },
    });
    const added: Entry[] = [];
    this.db.transaction(() => {
      for (const [key, value] of sets) this.db.run("insert into state (part, key, value) values (?, ?, ?) on conflict (part, key) do update set value = excluded.value", [fn.part, key, JSON.stringify(value)]);
      for (const { kind, fields } of adds) {
        const at = new Date().toISOString();
        const { lastInsertRowid } = this.db.run("insert into entries (at, part, kind, fields) values (?, ?, ?, ?)", [at, fn.part, kind, JSON.stringify(fields)]);
        added.push({ n: Number(lastInsertRowid), at, part: fn.part, kind, fields });
      }
    })();
    if (sets.size > 0) this.onState(fn.part, this.state(fn.part));
    for (const entry of added) {
      const wakes = this.kinds.get(`${entry.part}.${entry.kind}`)!.wakes ?? false;
      this.onEntry(entry, typeof wakes === "function" ? wakes(entry.fields, caller) : wakes);
    }
    return result;
  }

  /** The functions a slow agent has, as the tools every harness hands over: the same list the session's own three are in today. */
  tools(agent: number): HarnessTool[] {
    return [...this.fns.entries()]
      .filter(([, fn]) => fn.for !== "fast")
      .map(([name, fn]) => ({
        name: name.replace(".", "_"),
        description: fn.description,
        input: fn.input,
        run: async (input: any) => {
          try {
            const result = await this.call(name, input, { who: "slow", agent });
            return { text: typeof result === "string" ? result : JSON.stringify(result ?? "Done.") };
          } catch (error) {
            return { text: error instanceof Error ? error.message : String(error), error: true };
          }
        },
      }));
  }

  /** The lines of Fast's brief for the functions Fast may call. */
  brief() {
    return [...this.fns.entries()].filter(([, fn]) => fn.for !== "slow").map(([name, fn]) => `${name}(${Object.keys(fn.input).join(", ")})  // ${fn.brief ?? fn.description}`).join("\n");
  }

  /**
   * The sandbox half of every function Fast may call, as source: nothing but text crosses into the sandbox. Each is a
   * call that takes its input in the order it was registered, checks each value's type against the input's schema,
   * and records what was asked. Nothing is carried out while the script runs.
   */
  private prelude() {
    const fns = [...this.fns.entries()]
      .filter(([, fn]) => fn.for !== "slow")
      .map(([name, fn]) => {
        const schema = z.toJSONSchema(z.object(fn.input)) as { properties?: Record<string, { type?: string }>; required?: string[] };
        return { name, keys: Object.keys(fn.input), types: Object.fromEntries(Object.entries(schema.properties ?? {}).map(([key, value]) => [key, value.type ?? null])), required: schema.required ?? [] };
      });
    return `(() => {
      const asked = [];
      for (const fn of ${JSON.stringify(fns)}) {
        const [part, name] = fn.name.split(".");
        globalThis[part] ??= {};
        globalThis[part][name] = (...values) => {
          const input = {};
          fn.keys.forEach((key, at) => { if (values[at] !== undefined) input[key] = values[at]; });
          for (const key of fn.required) if (!(key in input)) throw new TypeError(fn.name + " needs " + key);
          for (const key in input) {
            const type = fn.types[key];
            if (type && type !== "array" && type !== "object" && typeof input[key] !== (type === "integer" ? "number" : type)) throw new TypeError(fn.name + ": " + key + " is not a " + type);
          }
          asked.push({ do: fn.name, input });
        };
      }
      globalThis.__done = () => JSON.stringify(asked);
    })()`;
  }

  /** Runs a script of Fast's in the sandbox and returns what it asked for. A script that throws asked for nothing. */
  async script(code: string): Promise<Asked[]> {
    const until = performance.now() + 300;
    const vm = await QuickJS.create({ wasm: await binary(), memoryLimit: 32 * 1024 * 1024, interruptHandler: () => performance.now() > until });
    try {
      vm.evalCode(this.prelude(), "<world>").dispose();
      vm.evalCode(code, "<script>").dispose();
      return JSON.parse(vm.evalCode("__done()", "<end>").consume((handle) => handle.toString()));
    } finally {
      vm.dispose();
    }
  }

  /** One run of Fast: the script in the sandbox, then each call carried out, in order. */
  async run(code: string, caller: Extract<Caller, { who: "fast" }>) {
    const asked = await this.script(code);
    for (const { do: name, input } of asked) await this.call(name, input, caller);
    return asked;
  }

  /** What a wake-up sends Fast for these entries: their lines as one text, and each image an entry carries after it. */
  wake(entries: Entry[]) {
    const images = entries.map((entry) => this.image(entry)).filter((image): image is string => Boolean(image));
    return { role: "user" as const, content: [{ type: "input_text" as const, text: entries.map((entry) => this.line(entry)).join("\n") }, ...images.map((image_url) => ({ type: "input_image" as const, image_url }))] };
  }
}
