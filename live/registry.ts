// Registered parts share an event stream, with state and events committed together.
import { line as coreLine } from "./world.ts";
import { coreEntry, Events } from "./events.ts";
import { Database } from "bun:sqlite";
import type { Intent } from "./sandbox.ts";
import { z } from "zod";
import type { HarnessTool } from "./harness.ts";
import { dirname } from "node:path";

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

/** A kind of entry: its fields, the line Fast reads, an image when it has one, and whether it wakes Fast. */
export interface Kind {
  fields?: Record<string, z.ZodType>;
  line: (fields: any) => string;
  /** A data URL or an address of an image the entry carries. */
  image?: (fields: any) => string | null;
  wakes?: boolean | ((fields: any, by: Caller) => boolean);
}

/** What a step of a part's work has: the part's state, to change, and a way to add entries. Both are saved together when the step ended. */
export interface Later {
  state: Record<string, any>;
  add: (kind: string, fields: Record<string, unknown>) => void;
}

/** Each result of work that outlasts a call is saved as one step of its part. */
type Publish = (change: (later: Later) => void) => Promise<void>;
type Follow = (publish: Publish) => Promise<void>;

/** What a function is run with, beside its input. */
export interface Now extends Later {
  caller: Caller;
  /** The `config` given where the part is enabled. */
  config: unknown;
  /** Starts work that outlasts the call, once what the call wrote is saved. */
  follow: (producer: Follow) => void;
}

/** A call of a script: the function's full name and its input. */
export interface Call {
  name: string;
  input: Record<string, unknown>;
}

/** A function of a part. */
export interface PartFunction {
  for: For;
  /** One line: the function's line in Fast's list of calls, and a slow agent's description of the tool. */
  description: string;
  input?: Record<string, z.ZodType>;
  /** For a function Fast may call: runs inside Fast's script, while it can still fail, with what the part last showed Fast and the script's earlier calls. Throws to refuse the call. What it returns is what the call answers in the script. */
  check?: (input: any, shown: Record<string, string>, earlier: Call[]) => unknown;
  run: (input: any, now: Now) => unknown | Promise<unknown>;
  /** The core's own only: the call as Fast's script writes it, for a call the sandbox itself provides. */
  syntax?: string;
  /** The program's own only: how a slow agent's harness is handed the tool, and the name a tool had before functions were registered. */
  tool?: Pick<HarnessTool, "always" | "reads"> & { name?: string };
}

export type Place = "navbar" | "panel" | "float";

/** A task as a part is shown it when it is given. */
export interface Given {
  /** What Fast wrote. */
  prompt: string;
  /** The page the message it was made from was said on, and what was marked then. */
  page: string | null;
  marked: string;
}

/** What the parts show Fast: by part, each title with its text. */
export type Shown = Record<string, Record<string, string>>;

/** A world model part: everything it declares, in one object. No field is required. */
export interface Part {
  /** Each field of its state, with the schema every write is checked against. A field's default is its value in a new session. */
  state?: Record<string, z.ZodType>;
  entries?: Record<string, Kind>;
  functions?: Record<string, PartFunction>;
  /** Its widgets, each with its place. Recorded only: placing is devpage's. */
  widgets?: Record<string, Place>;
  /** The lines the part adds to a task as it is given. */
  addToTask?: (state: Record<string, any>, task: Given) => string[] | void;
  /** What the part shows Fast at a wake-up, as titles with their text. */
  showFast?: (state: Record<string, any>) => Record<string, string> | Promise<Record<string, string>>;
}

/** What declares a part: the object, or a function that is given the schema builder and returns it. An extension's module cannot import the program's packages itself. */
export type Declare = Part | ((given: { z: typeof z }) => Part | Promise<Part>);

interface Fn extends PartFunction {
  part: string;
  name: string;
  input: Record<string, z.ZodType>;
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
const reserved = new Set("await break case catch class const continue debugger default delete do else enum export extends false finally for function if implements import in instanceof interface let new null package private protected public return static super switch this throw true try typeof var void while with yield arguments eval spawn say restart worldParts".split(" "));

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
  readonly widgets: { part: string; name: string; place: Place }[] = [];
  private readonly declared = new Map<string, Part & { config: unknown }>();
  /** The folder each part's text for the agents is in, in the order the parts were registered. */
  private readonly folders = new Map<string, string>();
  /** Called with each new entry and whether it wakes Fast. */
  onEntry: (entry: Entry, wakes: boolean) => void = () => {};
  /** Called with a part's whole state when it changed: what a page is sent. */
  onState: (part: string, state: Record<string, unknown>) => void = () => {};

  constructor(file: string | Database) {
    this.db = typeof file === "string" ? new Database(file) : file;
    this.events = new Events(this.db);
    this.events.install();
    this.db.run("create table if not exists state (part text not null, key text not null, value text not null, primary key (part, key))");
    // The part that holds the user's page was named `page` before it was the `devpage` part.
    this.db.run("update entries set part = 'devpage' where part = 'page'");
    this.db.run("update or ignore state set part = 'devpage' where part = 'page'");
    this.db.run("delete from state where part = 'page'");
  }

  /** Registers a part: the core's or an extension's, the same way. Nothing of a part that fails to declare itself is kept.
      `folder` is where its `voice.md`, `fast.md` and `slow.md` are, when it has text for the agents. */
  part(part: string, declare: Declare, config: unknown = undefined, folder?: string): void | Promise<void> {
    if (this.named.has(part)) throw new Error(`${part} is registered already`);
    const publish = (declared: Part) => {
      if (this.named.has(part)) throw new Error(`${part} is registered already`);
      const functions = new Map<string, Fn>();
      for (const [name, fn] of Object.entries(declared.functions ?? {})) functions.set(`${part}.${name}`, { ...fn, input: fn.input ?? {}, part, name });
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
      for (const [name, kind] of Object.entries(declared.entries ?? {})) this.kinds.set(`${part}.${name}`, { ...kind, part });
      for (const [name, fn] of functions) this.fns.set(name, fn);
      for (const [command, name] of commands) this.commands.set(command, name);
      for (const [name, place] of Object.entries(declared.widgets ?? {})) this.widgets.push({ part, name, place });
      this.declared.set(part, { ...declared, config });
      this.named.add(part);
      if (folder) this.folders.set(part, folder);
    };
    const declared = typeof declare === "function" ? declare({ z }) : declare;
    if (declared instanceof Promise) return declared.then(publish);
    publish(declared);
  }

  /** Loads an extension's world module: a file whose default export declares its part. */
  async load(part: string, file: string, config: unknown = undefined) {
    const module = await import(file);
    if (typeof module.default !== "function" && (typeof module.default !== "object" || module.default === null)) throw new Error(`${file} has no default export that declares a part`);
    await this.part(part, module.default, config, dirname(file));
  }

  /** The parts that may have text for the agents, each with the folder it is in. */
  texts() {
    return [...this.folders].map(([part, folder]) => ({ part, folder }));
  }

  /** The parts that are registered, by name. */
  parts() {
    return [...this.named];
  }

  /** A part's state, whole: what was saved, and its default for a declared field nothing was saved for. */
  state(part: string): Record<string, unknown> {
    const rows = this.db.query("select key, value from state where part = ?").all(part) as { key: string; value: string }[];
    const state = Object.fromEntries(rows.map(({ key, value }) => [key, JSON.parse(value)]));
    for (const [key, schema] of Object.entries(this.declared.get(part)?.state ?? {})) {
      if (key in state) continue;
      const initial = schema.safeParse(undefined);
      if (initial.success && initial.data !== undefined) state[key] = initial.data;
    }
    return state;
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
    return this.enqueue(fn.part, async () => {
      const producers: Follow[] = [];
      const config = this.declared.get(fn.part)?.config;
      const result = await this.change(fn.part, caller, later => fn.run(given, {
        ...later, caller, config, follow: producer => producers.push(producer),
      }));
      for (const produce of producers) {
        const publish: Publish = update => this.enqueue(fn.part, () => this.change(fn.part, caller, update));
        void Promise.resolve().then(() => produce(publish)).catch(error => console.error(`Following ${name}:`, error));
      }
      return result;
    });
  }

  /** Calls and subsequent results use the same per-part queue, including across awaits. */
  private async enqueue<T>(part: string, change: () => Promise<T>): Promise<T> {
    const previous = this.pending.get(part) ?? Promise.resolve();
    const run = previous.catch(() => {}).then(change);
    this.pending.set(part, run);
    try {
      return await run;
    } finally {
      if (this.pending.get(part) === run) this.pending.delete(part);
    }
  }

  /** One step of a part: what it changed of its state, each field checked against its schema, and the entries it added, saved together. */
  private async change<T>(part: string, caller: Caller, run: (later: Later) => T | Promise<T>) {
    const step = this.begin(part);
    const result = await run(step.later);
    step.end(caller);
    return result;
  }

  /** A step that does not wait for anything: for what a part does when nobody called one of its functions. */
  step<T>(part: string, run: (later: Later) => T, caller: Caller = { who: "page" }) {
    const step = this.begin(part);
    const result = run(step.later);
    step.end(caller);
    return result;
  }

  private begin(part: string) {
    if (!this.named.has(part)) throw new Error(`Unknown world part ${part}`);
    const schemas = this.declared.get(part)?.state ?? {};
    const before = this.state(part);
    const state = structuredClone(before);
    const adds: { kind: string; fields: Record<string, unknown> }[] = [];
    const later: Later = {
      state,
      add: (kind, fields) => {
        const declared = this.kinds.get(`${part}.${kind}`);
        if (!declared) throw new Error(`${part} has no entry kind ${kind}`);
        adds.push({ kind, fields: declared.fields ? z.object(declared.fields).parse(fields) : fields });
      },
    };
    const end = (caller: Caller) => {
      const sets: Record<string, unknown> = {};
      for (const key of new Set([...Object.keys(before), ...Object.keys(state)])) {
        if (JSON.stringify(state[key]) === JSON.stringify(before[key])) continue;
        if (!schemas[key]) throw new Error(`${part} has no state field ${key}`);
        sets[key] = schemas[key].parse(state[key]);
      }
      this.commit(part, sets, adds, caller);
    };
    return { later, end };
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
    const parsed = z.object(fn.input).safeParse(input);
    if (!parsed.success) throw new Error(parsed.error.issues.map(issue => (issue.path.length ? `${issue.path.join(".")}: ${issue.message}` : issue.message)).join("; "));
    return { fn, given: parsed.data as Record<string, unknown> };
  }

  /** One call of Fast's script, checked while the script can still fail: its input, and then the function's own check with what its part showed Fast and the script's earlier calls. */
  check(name: string, input: unknown, shown: Shown, earlier: Call[]) {
    const { fn, given } = this.checked(name, input, "fast");
    const value = fn.check?.(given, shown[fn.part] ?? {}, earlier);
    earlier.push({ name, input: given });
    return { input: given, value };
  }

  /** What every part shows Fast now. A part that cannot say shows nothing. */
  async shown(): Promise<Shown> {
    const all: Shown = {};
    for (const [part, declared] of this.declared) {
      if (!declared.showFast) continue;
      try {
        all[part] = await declared.showFast(this.state(part));
      } catch (error) {
        console.error(`${part} could not show Fast its part:`, error);
      }
    }
    return all;
  }

  /** What the parts add to a task as it is given: each part's lines as one paragraph, in the order the parts were registered. */
  added(task: Given): string[] {
    const paragraphs: string[] = [];
    for (const [part, declared] of this.declared) {
      const lines = declared.addToTask?.(this.state(part), task);
      if (lines?.length) paragraphs.push(lines.join("\n"));
    }
    return paragraphs;
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
            const result = await this.call(name, input, { who: "slow", agent });
            if (result && typeof result === "object" && "image" in result && typeof result.image === "string" && "text" in result && typeof result.text === "string") {
              const image = /^data:(image\/[a-z0-9.+-]+);base64,([A-Za-z0-9+/=]+)$/.exec(result.image);
              if (image) return { text: result.text, image: { mimeType: image[1], data: image[2] } };
            }
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
      .map(([name, fn]) => ({ name, description: fn.description, syntax: fn.syntax, schema: z.toJSONSchema(z.object(fn.input), { io: "input" }) }));
  }

  /** The parts' functions as the lines of Fast's list of calls. The core's own calls are written in its prompt. */
  brief() {
    return this.fastFunctions().filter(fn => !fn.syntax).map(({ name, description, schema }) =>
      `${`${binding(name).expression}(${Object.keys(schema.properties ?? {}).join(", ")})`.padEnd(27)} // ${description}`).join("\n");
  }

  /** The parts' functions as the script calls them. Each call is checked by the program as it is made, and is carried out only when the script ended. */
  prelude() {
    const functions = this.fastFunctions().filter(fn => !fn.syntax).map(fn => ({ ...binding(fn.name), full: fn.name, keys: Object.keys(fn.schema.properties ?? {}) }));
    return `(() => {
      const parts = globalThis.worldParts = Object.create(null);
      for (const fn of ${JSON.stringify(functions)}) {
        const part = parts[fn.part] ??= Object.create(null);
        if (fn.bare) globalThis[fn.part] = part;
        part[fn.name] = (...values) => {
          const input = {};
          fn.keys.forEach((key, at) => { if (values[at] !== undefined) input[key] = values[at]; });
          return __worldCall(fn.full, input);
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
