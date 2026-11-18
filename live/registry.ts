// Registered parts share an event stream, with state and events committed together.
import { line as coreLine } from "./world.ts";
import { coreEntry, Events } from "./events.ts";
import { Database } from "bun:sqlite";
import type { Intent } from "./sandbox.ts";
import { z } from "zod";
import type { HarnessTool } from "./harness.ts";

/** Who may call a function: Fast, slow agents, both of them, or a widget on the user's page. */
export type For = "fast" | "slow" | "both" | "page";

/** Who called: a run of Fast, with its hand-over and what its script read, a slow agent, or a widget on the user's page. */
export type Caller = { who: "fast"; run: number; handover: string | null; used: string[]; said?: import("./world.ts").Message[] } | { who: "slow"; agent: number | null } | { who: "page" };

/** One thing that happened, as the store keeps it. */
export interface Entry {
  n: number;
  originalNumber?: number;
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
  /** This function already has the session sandbox’s context-sensitive checks. */
  syntax?: string;
  input: Record<string, z.ZodType>;
  /** Stable harness spelling and transcript rendering for a tool with an existing interface. */
  tool?: Pick<HarnessTool, "name" | "always" | "reads">;
  run: (input: any, running: Running) => unknown | Promise<unknown>;
}

type Options = Pick<Fn, "for" | "description" | "brief" | "syntax" | "input" | "tool">;

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

/** Data needed to replay a script without installing or executing its packages. */
export interface FastFunction {
  name: string;
  description: string;
  syntax?: string;
  schema: z.core.JSONSchema.BaseSchema;
}

// A package id can be a digit, a keyword, a sandbox binding or a hyphenated name.
const identifier = /^[A-Za-z_$][\w$]*$/;
const reserved = new Set("await break case catch class const continue debugger default delete do else enum export extends false finally for function if implements import in instanceof interface let new null package private protected public return static super switch this throw true try typeof var void while with yield arguments eval spawn say restart worldParts page canvas".split(" "));

function binding(full: string) {
  const dot = full.indexOf(".");
  const part = full.slice(0, dot), name = full.slice(dot + 1);
  const bare = identifier.test(part) && !reserved.has(part) && !(part in globalThis) && !/^[mua]\d+$/.test(part);
  const owner = bare ? part : `worldParts[${JSON.stringify(part)}]`;
  const access = identifier.test(name) ? `.${name}` : `[${JSON.stringify(name)}]`;
  return { part, name, bare, expression: owner + access };
}

export class Registry {
  private readonly db: Database;
  readonly events: Events;
  private notices: { parts: Set<string>; entries: [Entry, boolean][] } | null = null;
  private readonly pending = new Map<string, Promise<unknown>>();
  private readonly kinds = new Map<string, Kind & { part: string }>();
  private readonly fns = new Map<string, Fn>();
  private readonly commands = new Map<string, string>();
  private readonly named = new Set<string>();
  readonly widgets: { part: string; name: string; place: string }[] = [];
  /** Called with each new entry and whether it wakes Fast. */
  onEntry: (entry: Entry, wakes: boolean) => void = () => {};
  /** Called with a part's whole state when it changed: what a page is sent. */
  onState: (part: string, state: Record<string, unknown>) => void = () => {};

  constructor(file: string | Database) {
    this.db = typeof file === "string" ? new Database(file) : file;
    this.events = new Events(this.db);
    this.events.install();
    this.db.run("create table if not exists state (part text not null, key text not null, value text not null, primary key (part, key))");
  }

  /** Registers a part: the core's or an extension's, the same way. */
  part(part: string, register: (as: Registering) => void) {
    if (this.named.has(part)) throw new Error(`${part} is registered already`);
    const kinds = new Map<string, Kind & { part: string }>();
    const functions = new Map<string, Fn>();
    const widgets: typeof this.widgets = [];
    const publish = () => {
      if (this.named.has(part)) throw new Error(`${part} is registered already`);
      const commands = new Map(this.commands);
      for (const [name, fn] of functions) {
        if (!fn.syntax) continue;
        if (commands.has(fn.name)) throw new Error(`The sandbox command ${fn.name} is registered twice`);
        commands.set(fn.name, name);
      }
      const tools = new Set<string>();
      for (const [name, fn] of [...this.fns, ...functions]) {
        if (fn.for !== "both" && fn.for !== "slow") continue;
        const tool = fn.tool?.name ?? name.replace(".", "_");
        if (tools.has(tool)) throw new Error(`The slow-agent tool ${tool} is registered twice`);
        tools.add(tool);
      }
      for (const [name, kind] of kinds) this.kinds.set(name, kind);
      for (const [name, fn] of functions) this.fns.set(name, fn);
      for (const [command, name] of commands) this.commands.set(command, name);
      this.widgets.push(...widgets);
      this.named.add(part);
    };
    const fn = (does: Fn["does"]) => (name: string, options: Options, run: Fn["run"]) => {
      if (does === "get" && options.for !== "slow") throw new Error(`${part}.${name}: a get is for slow agents only: Fast's script gets no answer from a call`);
      const full = `${part}.${name}`;
      if (functions.has(full) || this.fns.has(full)) throw new Error(`${full} is registered already`);
      functions.set(full, { part, name, does, run, ...options });
    };
    const registered: unknown = register({
      z,
      kind: (name, kind) => void kinds.set(`${part}.${name}`, { ...kind, part }),
      get: fn("get"),
      set: fn("set"),
      action: fn("action"),
      widget: (name, place) => void widgets.push({ part, name, place }),
    });
    if (registered instanceof Promise) return registered.then(publish);
    publish();
  }

  /** Loads an extension's world module: a file whose default export registers its part. */
  async load(part: string, file: string) {
    const module = await import(file);
    if (typeof module.default !== "function") throw new Error(`${file} has no default export that registers a part`);
    await this.part(part, module.default);
  }

  /** The parts that are registered, by name. */
  parts() {
    return [...this.named];
  }

  /** A part's state, whole. */
  state(part: string): Record<string, unknown> {
    const rows = this.db.query("select key, value from state where part = ?").all(part) as { key: string; value: string }[];
    return Object.fromEntries(rows.map(({ key, value }) => [key, JSON.parse(value)]));
  }

  /** Every entry after the one with this number, oldest first. */
  since(n: number): Entry[] {
    return this.events.since(n);
  }

  /** An entry as the line Fast reads. */
  line(entry: Entry) {
    const kind = this.kinds.get(`${entry.part}.${entry.kind}`);
    const fallback = `${entry.part}.${entry.kind}`;
    try {
      const core = coreEntry(entry);
      const rendered = kind ? kind.line(entry.fields) : core ? coreLine(core) : fallback;
      return typeof rendered === "string" ? rendered : fallback;
    } catch {
      // Earlier records can have a different shape from the current package.
      return fallback;
    }
  }

  /** The image an entry carries, when it has one. */
  image(entry: Entry) {
    try {
      const image = this.kinds.get(`${entry.part}.${entry.kind}`)?.image?.(entry.fields);
      return typeof image === "string" ? image : null;
    } catch {
      return null;
    }
  }

  /**
   * Calls a function for a caller. Its input is checked, and what it wrote, state and entries, is committed in one
   * transaction once it ended: a function that throws has written nothing.
   */
  async call(name: string, input: unknown, caller: Caller) {
    const { fn, given } = this.checked(name, input, caller.who);
    // A function may await external work. Serialize calls per part across that
    // await while letting unrelated parts commit on the same SQLite connection.
    const previous = this.pending.get(fn.part);
    const run = (previous ?? Promise.resolve()).catch(() => {}).then(() => this.execute(fn, given, caller));
    this.pending.set(fn.part, run);
    try {
      return await run;
    } finally {
      if (this.pending.get(fn.part) === run) this.pending.delete(fn.part);
    }
  }

  private async execute(fn: Fn, given: unknown, caller: Caller) {
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
    this.commit(fn.part, Object.fromEntries(sets), adds, caller);
    return result;
  }

  /** A synchronous change can include several parts. Publish their final
   * snapshots only after the outermost transaction has committed. */
  atomic<T>(change: () => T): T {
    if (this.notices) return change();
    const notices = { parts: new Set<string>(), entries: [] as [Entry, boolean][] };
    this.notices = notices;
    let result: T;
    try {
      result = this.db.transaction(change)();
    } finally {
      this.notices = null;
    }
    for (const part of notices.parts) this.onState(part, this.state(part));
    for (const [entry, wakes] of notices.entries) this.onEntry(entry, wakes);
    return result;
  }

  commit(part: string, sets: Record<string, unknown>, adds: { kind: string; fields: Record<string, unknown> }[], caller: Caller = { who: "page" }) {
    if (!this.named.has(part)) throw new Error(`Unknown world part ${part}`);
    return this.atomic(() => {
      for (const [key, value] of Object.entries(sets)) {
        const written = this.db.run("insert into state (part, key, value) values (?, ?, ?) on conflict (part, key) do update set value = excluded.value where value <> excluded.value", [part, key, JSON.stringify(value)]);
        if (written.changes) this.notices!.parts.add(part);
      }
      return adds.map(({ kind, fields }) => {
        const registered = this.kinds.get(`${part}.${kind}`);
        if (!registered) throw new Error(`${part} has no entry kind ${kind}`);
        const entry = this.events.append(part, kind, fields);
        const image = this.image(entry);
        if (image) this.events.keepImage(entry, image);
        const wakes = registered.wakes;
        let wake = wakes === true;
        if (typeof wakes === "function") {
          try { wake = wakes(fields, caller) === true; }
          catch { /* A failed notification predicate leaves this entry available on the next wake. */ }
        }
        this.notices!.entries.push([entry, wake]);
        return entry;
      });
    });
  }

  /** Both forms produced by the sandbox resolve to the registered function. */
  requested(intent: Intent) {
    if (intent.do === "world") return { name: intent.name, input: intent.input };
    const { do: command, ...input } = intent;
    const name = this.commands.get(command);
    if (!name) throw new Error(`There is no registered sandbox command ${command}`);
    return { name, input };
  }

  /** Check the whole script before carrying any of its intents out. */
  checked(name: string, input: unknown, who: Caller["who"]) {
    const fn = this.fns.get(name);
    if (!fn) throw new Error(`there is no function ${name}`);
    if (fn.for !== who && !(fn.for === "both" && who !== "page")) throw new Error(`${name} is for ${fn.for}, not for ${who}`);
    return { fn, given: z.object(fn.input).parse(input) };
  }

  /** The functions a slow agent has, as the tools every harness hands over: the same list the session's own three are in today. */
  tools(agent: number | null): HarnessTool[] {
    return [...this.fns.entries()]
      .filter(([, fn]) => fn.for === "slow" || fn.for === "both")
      .map(([name, fn]) => ({
        name: name.replace(".", "_"),
        ...fn.tool,
        description: fn.description,
        input: fn.input,
        run: async (input: any) => {
          try {
            if (agent === null && fn.does !== "get") throw new Error("A base agent has no active task.");
            const result = await this.call(name, input, { who: "slow", agent });
            return { text: typeof result === "string" ? result : JSON.stringify(result ?? "Done.") };
          } catch (error) {
            return { text: error instanceof Error ? error.message : String(error), error: true };
          }
        },
      }));
  }

  /** This description is given to the sandbox and kept for replay in the eval. */
  fastFunctions(): FastFunction[] {
    return [...this.fns].filter(([, fn]) => fn.for === "fast" || fn.for === "both")
      .map(([name, fn]) => ({ name, description: fn.brief ?? fn.description, syntax: fn.syntax, schema: z.toJSONSchema(z.object(fn.input)) }));
  }

  brief() {
    return this.fastFunctions().map(({ name, syntax, description, schema }) =>
      `${syntax ?? `${binding(name).expression}(${Object.keys(schema.properties ?? {}).join(", ")})`}  // ${description}`).join("\n");
  }

  prelude() {
    const functions = this.fastFunctions().filter(fn => !fn.syntax).map(fn => ({
      ...binding(fn.name), full: fn.name, keys: Object.keys(fn.schema.properties ?? {}), required: fn.schema.required ?? [],
      types: Object.fromEntries(Object.entries(fn.schema.properties ?? {}).map(([key, property]) =>
        [key, typeof property === "object" ? property.type : undefined])),
    }));
    // Zod checks every collected input before the service carries out the first
    // intent. Preserve the order of these calls among the built-ins.
    return `(() => {
      const parts = globalThis.worldParts = Object.create(null);
      for (const fn of ${JSON.stringify(functions)}) {
        const part = parts[fn.part] ??= Object.create(null);
        if (fn.bare) globalThis[fn.part] = part;
        part[fn.name] = (...values) => {
          const input = {};
          fn.keys.forEach((key, at) => { if (values[at] !== undefined) input[key] = values[at]; });
          for (const key of fn.required) if (!(key in input)) throw new TypeError(fn.name + " needs " + key);
          for (const key of Object.keys(input)) {
            const type = fn.types[key];
            if (!type) continue;
            const value = input[key];
            const actual = value === null ? "null" : Array.isArray(value) ? "array" : typeof value;
            const allowed = Array.isArray(type) ? type : [type];
            if (!allowed.includes(actual) && !(allowed.includes("integer") && Number.isInteger(value))) {
              throw new TypeError(fn.name + ": " + key + " needs " + allowed.join(" or "));
            }
          }
          __worldCall(fn.full, input);
        };
      }
    })()`;
  }

  /** What a wake-up sends Fast for these entries: their lines as one text, and each image an entry carries after it. */
  wake(entries: Entry[], text = entries.map(entry => this.line(entry)).join("\n")) {
    const saved = this.events.images(entries.map(entry => entry.n));
    const images = entries.map((entry) => saved.get(entry.n) ?? this.image(entry)).filter((image): image is string => Boolean(image));
    return { role: "user" as const, content: [{ type: "input_text" as const, text }, ...images.map((image_url) => ({ type: "input_image" as const, image_url }))] };
  }
}
