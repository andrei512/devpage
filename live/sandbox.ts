// Where Fast's scripts run: QuickJS, with nothing of this machine in reach. A script gets the part of the world Fast
// holds as variables (`m41`, `u12`, `a2`) and the calls: the core's (`spawn`, `say`, ...) and each part's functions.
// The calls do nothing while the script runs: they are checked and collected, and the service carries them out when
// the script ended without an error. So a script that throws has done nothing.
import { readFileSync } from "node:fs";
import type { Call, Registry, Shown } from "./registry.ts";
import { QuickJS } from "quickjs-wasi";

/** What a script sees. Every id in it is one Fast was shown. */
export interface Scope {
  messages: { id: number; text: string; page: string | null; marked: string }[];
  updates: { id: number; text: string }[];
  agents: number[];
  /** Enabled routes at the start of this run. */
  routes?: string[];
}

/** One thing a script asked for. */
export type Intent =
  /** A call of a part's function, with its input as the program checked it. */
  | { do: "world"; name: string; input: Record<string, unknown> }
  | { do: "say"; text: string }
  /** `route`: the route the script named for the new agent, as it wrote it. */
  | { do: "spawn"; prompt: string; route?: string }
  | { do: "send"; agent: number; prompt: string }
  | { do: "fork"; agent: number; prompt: string }
  | { do: "restart"; force: boolean };

export interface Ran {
  intents: Intent[];
  /** The messages and updates the script read, as `m41`, `u12`. */
  used: string[];
  /** What the script wrote with `console.log`: for the log only. */
  printed: string[];
  ms: number;
}

/** How long a script may run. */
const LIMIT_MS = 300;

// The script's world, built inside the sandbox from the scope's JSON. A message or an update is an object that turns
// into its words in a string; any other property is an error that says what exists, so a wrong guess is caught and
// never becomes the text "undefined" in a prompt.
const PRELUDE = `(() => {
  const scope = JSON.parse(__scope);
  const intents = [], used = [], printed = [];
  const text = (value, what) => {
    if (value === undefined || value === null) throw new TypeError(what + " needs a text");
    const given = String(value);
    if (!given.trim()) throw new TypeError(what + " got an empty text");
    return given;
  };
  const thing = (name, words, more, hint) => new Proxy({}, {
    get(_, key) {
      if (key === Symbol.toPrimitive || key === "toString" || key === "valueOf" || key === "toJSON") return () => { used.push(name); return words; };
      if (typeof key === "symbol") return undefined;
      if (key in more) { used.push(name); return more[key]; }
      throw new TypeError(name + " has no ." + key + ": " + hint);
    },
    set() { throw new TypeError(name + " cannot be changed"); },
  });
  for (const m of scope.messages) globalThis["m" + m.id] = thing("m" + m.id, m.text, { page: m.page ?? "", marked: m.marked }, "use \${m" + m.id + "} for the words, m" + m.id + ".page and m" + m.id + ".marked");
  for (const u of scope.updates) globalThis["u" + u.id] = thing("u" + u.id, u.text, {}, "use \${u" + u.id + "} for its text");
  for (const id of scope.agents) globalThis["a" + id] = Object.freeze({
    send(prompt) { intents.push({ do: "send", agent: id, prompt: text(prompt, "a" + id + ".send") }); },
    fork(prompt) { intents.push({ do: "fork", agent: id, prompt: text(prompt, "a" + id + ".fork") }); },
  });
  globalThis.spawn = (prompt, route) => { intents.push(route === undefined || route === null ? { do: "spawn", prompt: text(prompt, "spawn") } : { do: "spawn", prompt: text(prompt, "spawn"), route: text(route, "spawn's route") }); };
  globalThis.say = (words) => { intents.push({ do: "say", text: text(words, "say") }); };
  globalThis.restart = (force) => { intents.push({ do: "restart", force: force === true }); };
  globalThis.console = { log: (...values) => { printed.push(values.map(String).join(" ")); } };

  // A part's function: the program checks the call now, while the script can still fail, and answers what the call answers.
  globalThis.__worldCall = (name, input) => {
    const checked = JSON.parse(__check(name, JSON.stringify(input)));
    if ("error" in checked) throw new Error(name + ": " + checked.error);
    intents.push({ do: "world", name, input: checked.input });
    return checked.value;
  };
  globalThis.__done = () => {
    return JSON.stringify({ intents, used: [...new Set(used)], printed });
  };
})()`;

let compiled: Promise<WebAssembly.Module> | null = null;

/** The QuickJS binary, compiled once. */
function binary() {
  compiled ??= WebAssembly.compile(readFileSync(new URL(import.meta.resolve("quickjs-wasi/quickjs.wasm"))));
  return compiled;
}

/** Runs one script. `shown` is what the parts last showed Fast: what their functions check a call against. Throws what the script threw, as an Error whose message Fast is given. */
export async function run(script: string, scope: Scope, registry?: Registry, shown: Shown = {}): Promise<Ran> {
  const began = performance.now();
  const until = began + LIMIT_MS;
  const vm = await QuickJS.create({ wasm: await binary(), memoryLimit: 32 * 1024 * 1024, interruptHandler: () => performance.now() > until });
  try {
    vm.newString(JSON.stringify(scope)).consume((handle) => vm.setProp(vm.global, "__scope", handle));
    const earlier: Call[] = [];
    vm.newFunction("__check", (name, input) => {
      let answer: { input: unknown; value?: unknown } | { error: string };
      try {
        if (!registry) throw new Error("no part has this function");
        answer = registry.check(name.toString(), JSON.parse(input.toString()), shown, earlier);
      } catch (error) {
        answer = { error: error instanceof Error ? error.message : String(error) };
      }
      return vm.newString(JSON.stringify(answer));
    }).consume((handle) => vm.setProp(vm.global, "__check", handle));
    vm.evalCode(PRELUDE, "<world>").dispose();
    if (registry) vm.evalCode(registry.prelude(), "<registered functions>").dispose();
    try {
      vm.evalCode(script, "<script>").dispose();
      const done = vm.evalCode("__done()", "<end>").consume((handle) => handle.toString());
      const result = JSON.parse(done) as Ran;
      for (const intent of result.intents) {
        if (intent.do === "spawn" && intent.route && scope.routes &&
            !scope.routes.some(route => route.toLowerCase() === intent.route!.trim().toLowerCase())) {
          throw new Error(intent.route + " is not an enabled route");
        }
        // A part's call was checked as the script made it.
        if (registry && intent.do !== "world") {
          const call = registry.requested(intent);
          registry.checked(call.name, call.input, "fast");
        }
      }
      return { ...result, ms: Math.round(performance.now() - began) };
    } catch (error: any) {
      const name = typeof error?.name === "string" ? error.name : "Error";
      const message = typeof error?.message === "string" ? error.message : String(error);
      if (/interrupted/.test(message)) throw new Error(`The script ran longer than ${LIMIT_MS} ms and was stopped.`);
      // A name Fast was never shown: the usual wrong id.
      const missing = /^'?(\w+)'? is not defined/.exec(message);
      if (name === "ReferenceError" && missing) throw new Error(`${missing[1]} does not exist. Use only the ids you were shown.`);
      throw new Error(`${name}: ${message}`);
    }
  } finally {
    vm.dispose();
  }
}
