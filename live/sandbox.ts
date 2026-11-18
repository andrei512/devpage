// Where Fast's scripts run: QuickJS, with nothing of this machine in reach. A script gets the part of the world Fast
// holds as variables (`m41`, `u12`, `a2`, `page`) and a few calls (`spawn`, `say`, ...). The calls do nothing while
// the script runs: they are collected, and the service carries them out when the script ended without an error. So
// a script that throws has done nothing.
import { readFileSync } from "node:fs";
import { QuickJS } from "quickjs-wasi";

/** What a script sees. Every id in it is one Fast was shown. */
export interface Scope {
  messages: { id: number; text: string; page: string | null; marked: string }[];
  updates: { id: number; text: string }[];
  agents: number[];
  /** The page the user is on, with its text as the user sees it; null when no page is open. */
  page: { address: string; text: string } | null;
  /** Every page's address: where `page.go` may go. */
  pages: string[];
  /** The user's messages of the hand-over that woke Fast: a script that gives work to an agent must use one of them. */
  handover: number[];
  /** The scratch pads there are, as addresses. Left out where there are none to know of. */
  pads?: string[];
}

/** One thing a script asked for. */
export type Intent =
  | { do: "say"; text: string }
  /** `route`: the route the script named for the new agent, as it wrote it. */
  | { do: "spawn"; prompt: string; route?: string }
  | { do: "send"; agent: number; prompt: string }
  | { do: "fork"; agent: number; prompt: string }
  | { do: "go"; address: string }
  | { do: "scroll"; where: "up" | "down" | "top" | "bottom" }
  | { do: "panel"; open: boolean }
  | { do: "point"; quote: string }
  | { do: "select"; quote: string }
  /** Opens the canvas on a scratch pad. `title` when the script made the pad's address from an idea's title. */
  | { do: "show"; address: string; title?: string }
  /** Closes the canvas. */
  | { do: "close" }
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
  // A task as it is given: what the script wrote, and where the user is and what the user has marked, which every task needs.
  const task = (prompt, what) => {
    const written = text(prompt, what);
    const now = scope.messages.reduce((last, m) => (!last || m.id > last.id ? m : last), null);
    const where = (scope.page && scope.page.address) || (now && now.page);
    return written + (where ? "\\n\\nThe page the user is on: " + where : "") + (now && now.marked ? '\\nMarked on it: "' + now.marked + '"' : "");
  };
  for (const id of scope.agents) globalThis["a" + id] = Object.freeze({
    send(prompt) { intents.push({ do: "send", agent: id, prompt: task(prompt, "a" + id + ".send") }); },
    fork(prompt) { intents.push({ do: "fork", agent: id, prompt: task(prompt, "a" + id + ".fork") }); },
  });
  globalThis.spawn = (prompt, route) => { intents.push(route === undefined || route === null ? { do: "spawn", prompt: task(prompt, "spawn") } : { do: "spawn", prompt: task(prompt, "spawn"), route: text(route, "spawn's route") }); };
  globalThis.say = (words) => { intents.push({ do: "say", text: text(words, "say") }); };
  globalThis.restart = (force) => { intents.push({ do: "restart", force: force === true }); };
  globalThis.console = { log: (...values) => { printed.push(values.map(String).join(" ")); } };

  const flat = (value) => String(value).replace(/\\s+/g, " ").trim();
  let moved = false;
  // A part is named by words of the page. They must be on it, once: checked here, while the script can still fail.
  const part = (quote, what) => {
    const wanted = flat(text(quote, what));
    if (moved) return wanted;
    if (!scope.page) throw new Error(what + ": no page is open");
    const all = flat(scope.page.text);
    const first = all.indexOf(wanted);
    if (first < 0) throw new Error(what + ': the page has no text "' + wanted + '". Copy the words exactly from the page text you were shown.');
    if (all.indexOf(wanted, first + 1) >= 0) throw new Error(what + ': "' + wanted + '" is on the page more than once. Use more words, so that it names one place.');
    return wanted;
  };
  // A scratch pad's address is made from its idea's title, so the same idea is the same pad.
  const made = {};
  const PADS = "pads/";
  globalThis.canvas = Object.freeze({
    pad(title) {
      const given = text(title, "canvas.pad").replace(/\\s+/g, " ").trim();
      const slug = given.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 60);
      if (!slug) throw new TypeError("canvas.pad takes the idea's title, in a few words");
      const address = PADS + slug + ".md";
      made[address] = given;
      return address;
    },
    show(address) {
      const given = text(address, "canvas.show").replace(/^\\/*/, "");
      if (!(given in made) && !(scope.pads || []).includes(given)) throw new Error("canvas.show: there is no scratch pad " + given + ". Show one from the list of scratch pads, or make a new one: canvas.show(canvas.pad(\\"The idea's title\\")).");
      moved = true;
      intents.push(given in made ? { do: "show", address: given, title: made[given] } : { do: "show", address: given });
    },
    close() {
      moved = true;
      intents.push({ do: "close" });
    },
  });
  globalThis.page = Object.freeze({
    go(address) {
      const given = text(address, "page.go").replace(/^\\/*/, "/");
      if (!scope.pages.includes(given.split("#")[0])) throw new Error("page.go: no page has the address " + given.split("#")[0] + ". The addresses are in the list of pages.");
      moved = true;
      intents.push({ do: "go", address: given });
    },
    scroll(where) {
      if (!["up", "down", "top", "bottom"].includes(where)) throw new TypeError('page.scroll takes "up", "down", "top" or "bottom"');
      intents.push({ do: "scroll", where });
    },
    panel(open) {
      if (typeof open !== "boolean") throw new TypeError("page.panel takes true to open the side panel, false to close it");
      intents.push({ do: "panel", open });
    },
    point(quote) { intents.push({ do: "point", quote: part(quote, "page.point") }); },
    select(quote) { intents.push({ do: "select", quote: part(quote, "page.select") }); },
  });

  globalThis.__done = () => {
    const gives = intents.some((intent) => intent.do === "spawn" || intent.do === "send" || intent.do === "fork");
    const mine = scope.handover.map((id) => "m" + id);
    if (gives && mine.length > 0 && !mine.some((name) => used.includes(name))) {
      throw new Error("The prompt for the agent does not have what the user just said. The user said " + mine.join(", ") + ": put it in the prompt as " + mine.map((name) => "\${" + name + "}").join(" ") + ".");
    }
    return JSON.stringify({ intents, used: [...new Set(used)], printed });
  };
})()`;

let compiled: Promise<WebAssembly.Module> | null = null;

/** The QuickJS binary, compiled once. */
function binary() {
  compiled ??= WebAssembly.compile(readFileSync(new URL(import.meta.resolve("quickjs-wasi/quickjs.wasm"))));
  return compiled;
}

/** Runs one script. Throws what the script threw, as an Error whose message Fast is given. */
export async function run(script: string, scope: Scope): Promise<Ran> {
  const began = performance.now();
  const until = began + LIMIT_MS;
  const vm = await QuickJS.create({ wasm: await binary(), memoryLimit: 32 * 1024 * 1024, interruptHandler: () => performance.now() > until });
  try {
    vm.newString(JSON.stringify(scope)).consume((handle) => vm.setProp(vm.global, "__scope", handle));
    vm.evalCode(PRELUDE, "<world>").dispose();
    try {
      vm.evalCode(script, "<script>").dispose();
      const done = vm.evalCode("__done()", "<end>").consume((handle) => handle.toString());
      return { ...(JSON.parse(done) as Pick<Ran, "intents" | "used" | "printed">), ms: Math.round(performance.now() - began) };
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
