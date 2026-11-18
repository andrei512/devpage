import { SessionStore } from "./store.ts";
// The live session's service. devpage starts it and passes it every request and WebSocket under the extension's
// `live/` address. It holds the voice session, Fast, the slow agents, the world and the tasks, and keeps the keys:
// none reaches the browser. Voice hands everything the user says to Fast; Fast answers with a script, and this file carries
// out what the script asked for: a line for Voice, work for an agent, a move of the user's page. What the agents publish goes
// into the world, and Fast decides what the user hears of it.
import { randomUUID } from "node:crypto";
import { existsSync, readFileSync, readdirSync, rmSync, mkdirSync, statSync, watch, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { isAbsolute, join } from "node:path";
import type { ServerWebSocket } from "bun";
import { Agents, registerAgentFunctions } from "./agents.ts";
import { ClaudeCode } from "./claude.ts";
import { Codex } from "./codex.ts";
import type { Harness } from "./harness.ts";
import { checked, enabledRoutes, find, rules, settings, told, type Enabled, type Route } from "./routes.ts";
import { NEEDS, provider } from "./provider.ts";
import { Recording } from "./recording.ts";
import { registerCore, type CorePart } from "./core-parts.ts";
import { type Registering, Registry } from "./registry.ts";
import { BACKEND, Fast, moved, type Carried, type Seen } from "./fast.ts";
import { everywhere, here as project, HOME, stateOf } from "./home.ts";
import { map } from "./pages.ts";
import { Log } from "./log.ts";
import type { Intent } from "./sandbox.ts";
import { Tasks, type Part } from "./tasks.ts";
import { Voice, type Said } from "./voice.ts";
import { line, World, type Message, type Update } from "./world.ts";

const PORT = Number(process.env.DEVPAGE_PORT);
const TOKEN = process.env.DEVPAGE_TOKEN;
const DEVPAGE = process.env.DEVPAGE_URL!;
const ROOT = process.env.DEVPAGE_ROOT!;
/** Where the session keeps its own files: the log, the world, the tasks, the agents, the scratch pads. Outside the project. */
const STATE = stateOf(ROOT);
mkdirSync(STATE, { recursive: true });
/** The session history shares its database with registered world parts. */
const store = new SessionStore(STATE);
const registry = new Registry(store.db);
const log = new Log(store);
log.write("service", "service_started", { text: "The service started." });

const complain = (text: string) => log.write("service", "settings_problem", { text });
/** The settings: the project's own, and under them the ones for every project. A project that sets something replaces it whole. */
const SETTINGS = { ...everywhere(complain), ...project(complain) };
/** Where the voice and Fast run, and the key: OpenAI or Azure, from devpage's environment. The key reaches no page. */
const PROVIDER = provider(process.env);
/** Whether the user's voice is saved: only when the `record` setting says so. */
const RECORD = SETTINGS.record === true;
/** The recording of the voice session that is on, when the user's voice is saved. */
let recording: Recording | null = null;
/** Fast's model: the `fast` setting names another, and on Azure the deployment. */
const FAST_MODEL = typeof SETTINGS.fast === "string" && SETTINGS.fast ? SETTINGS.fast : BACKEND;

type Socket = ServerWebSocket<undefined>;
const sockets = new Set<Socket>();
/** The tab the user used last: page actions go there. */
let active: Socket | null = null;
/** The tab whose microphone the voice session hears. */
let owner: Socket | null = null;

let voice: Voice | null = null;
let voiceState: "off" | "starting" | "on" = "off";
let problem: string | null = null;
interface Page {
  address: string;
  fragment: string;
  sources: string[];
}
/** The page each open tab shows. Every tab reports its own, also on a live update while it is in the background. */
const pages = new Map<Socket, Page>();
/** The address the user was last known to be on. */
const savedPage = registry.state("page");
let known = savedPage.address as string | null ?? null;
/** What is marked, by the user or by a script's `page.select`: always a part of the page the user is on. */
let subject = savedPage.subject as Part | null ?? null;
/** Who marked it: the page draws the user's mark in the colour of the user's voice and the assistant's in its own. Means nothing while nothing is marked. */
let author = savedPage.by as "assistant" | "user" ?? "user";
/** Until when a change of page is a script's doing, not the user's: a script asked the page to go somewhere a moment ago. */
let moving = 0;
/** The last thing heard or said, as it grows. */
let heard = { who: "me" as "me" | "ai", text: "", at: 0 };
/** When the user was last heard speaking. */
let spoke = 0;

/** Where the scratch pads are: Markdown files that are not in the project, one per idea. */
const PADS = join(STATE, "pads");
/** A pad's address in the session: `pads/` and its file's name. No page of the project has an address without a `/` in front. */
const PAD = /^pads\/[a-z0-9][a-z0-9-]*\.md$/;
mkdirSync(PADS, { recursive: true });
/** The scratch pad on the canvas, with a number that changes whenever its file does; null while the canvas is closed. */
type Canvas = { pad: string; version: number };
let canvas = registry.state("canvas").canvas as Canvas | null ?? null;
if (canvas && (!PAD.test(canvas.pad) || !existsSync(join(PADS, canvas.pad.slice(5))))) canvas = null;

/** A pad's file, from its address. */
const padFile = (address: string) => join(PADS, address.slice("pads/".length));

/** The scratch pads there are, newest first, each with its idea's title: its first heading. */
function pads() {
  try {
    return readdirSync(PADS)
      .filter((name) => PAD.test(`pads/${name}`))
      .map((name) => {
        const file = join(PADS, name);
        const title = /^#\s+(.+)$/m.exec(readFileSync(file, "utf8").slice(0, 2000))?.[1].trim() ?? name;
        return { address: `pads/${name}`, title, changed: statSync(file).mtimeMs };
      })
      .sort((a, b) => b.changed - a.changed)
      .slice(0, 30)
      .map(({ address, title }) => ({ address, title }));
  } catch {
    return [];
  }
}

/** Opens the canvas on a pad. A pad that is not there yet is made, with its idea's title, so there is something to show at once. */
function showPad(address: string, title: string | undefined, by: "me" | "fast") {
  if (!PAD.test(address)) throw new Error(`${address} is not a scratch pad`);
  const file = padFile(address);
  if (!existsSync(file)) {
    if (by === "me") throw new Error(`there is no scratch pad ${address}`);
    writeFileSync(file, `# ${title ?? address.slice("pads/".length, -3).replace(/-/g, " ")}\n`);
  }
  canvas = { pad: address, version: Date.now() };
  // The pad is the page the user is on now, and a mark left on the page under it is not about it.
  known = address;
  subject = null;
  world.add({ kind: "page", by, text: by === "fast" ? `Fast opened the canvas on the scratch pad ${address}.` : `The user opened the canvas on the scratch pad ${address}.` });
  publish("page", "canvas");
}

/** Closes the canvas: the page under it is the page the user is on again. */
function closePad(by: "me" | "fast") {
  if (!canvas) return false;
  canvas = null;
  subject = null;
  known = here()?.address ?? null;
  world.add({ kind: "page", by, text: by === "fast" ? "Fast closed the canvas." : "The user closed the canvas themselves." });
  publish("page", "canvas");
  return true;
}

// The pad on the canvas is drawn again when its file changes: devpage does not watch a file outside the project.
let padChanged: ReturnType<typeof setTimeout> | undefined;
try {
  watch(PADS, (_event, name) => {
    if (!canvas || (name && `pads/${name}` !== canvas.pad)) return;
    clearTimeout(padChanged);
    padChanged = setTimeout(() => {
      if (!canvas) return;
      if (!existsSync(padFile(canvas.pad))) return void registry.atomic(() => closePad("fast"));
      canvas = { ...canvas, version: Date.now() };
      publish("canvas");
    }, 150);
  });
} catch (error) {
  log.write("service", "pads_not_watched", { text: error instanceof Error ? error.message : String(error) });
}

/** The page the user is on: the pad on the canvas while it is open, or else the page in the tab with the microphone, or in the tab the user used last. */
function here(): Page | null {
  if (canvas) return { address: canvas.pad, fragment: "", sources: [canvas.pad] };
  const tab = owner ?? active;
  return (tab && pages.get(tab)) ?? null;
}

/** Whether a part is on a page. */
function shows(page: Page | null, part: Part) {
  const address = (source: string) => source.replace(/^\/*/, "/");
  return Boolean(page?.sources.some((source) => address(source) === address(part.page)));
}

/** Brings what is known in line with the page the user is on: a page the user opened goes into the world, and a mark left on another page is dropped. */
function settle() {
  const page = here();
  if (page && page.address !== known) {
    // A page that changed without a script having just asked for it is one the user opened themselves.
    if (known !== null && Date.now() > moving) world.add({ kind: "page", by: "me", text: `The user opened ${page.address}.` });
    known = page.address;
    publish("page");
  }
  if (subject && page && !shows(page, subject)) {
    subject = null;
    publish("page");
  }
}

const tasks = new Tasks(store, () => publish("work"));

/** The world's registered parts, with their state and entries in one store. A page is sent a part's state when it changes. */
registerCore(registry, coreFunctions());
registry.onState = (part, state) => tell({ type: "state", part, state });
// The `world` setting names parts and the module that registers each: a file in the project, or anywhere by its full path.
for (const [part, file] of Object.entries(SETTINGS.world && typeof SETTINGS.world === "object" ? SETTINGS.world : {})) {
  if (typeof file !== "string") continue;
  await registry.load(part, isAbsolute(file) ? file : join(ROOT, file)).then(
    () => {
      log.write("service", "world_part", { text: part });
      tell({ type: "state", part, state: registry.state(part) });
    },
    (error) => log.write("service", "world_part_failed", { text: `${part}: ${error instanceof Error ? error.message : String(error)}` }),
  );
}

registry.onEntry = (entry, wakes) => {
  log.write("world", entry.kind, { part: entry.part, text: registry.line(entry), n: entry.n });
  if (wakes) fast?.entry(`${entry.part}.${entry.kind}`, entry.n);
};

const world = new World(registry);

/** The agent whose chat each page has open: that page gets the agent's session as it happens. */
const watching = new Map<Socket, number>();
// The harnesses there are, by the name the settings give each. Nothing below this knows what any of them is.
const every = new Map<string, Harness>([
  ["claude-code", new ClaudeCode()],
  ["codex", new Codex()],
]);
// The settings say which are on, with which models. Without them there is one: Claude Code on its own default model.
const ONE: Enabled = [["claude-code", []]];
/** The routes that are enabled; the first is the default. As the settings give them until the harnesses said what they have. */
let routes: Route[] = enabledRoutes(settings(SETTINGS.harnesses, complain) ?? ONE, (harness) => every.has(harness), complain);
if (routes.length === 0) routes = enabledRoutes(ONE, () => true, complain);
const enabled = new Map([...every].filter(([name]) => routes.some((route) => route.harness === name)));
log.write("service", "routes", { text: routes.map((route) => route.name).join(", ") });
// Each harness is asked which models it has: a route to a model that is not there is dropped, and the others say what
// they are for. The default route is kept as it is, so that there always is one.
void checked(routes, enabled, complain).then((sure) => {
  routes = sure.some((route) => route.name === routes[0].name) ? sure : [routes[0], ...sure];
  log.write("service", "routes_checked", { text: routes.map((route) => route.name).join(", ") });
});

const agents = new Agents(ROOT, PADS, enabled, routes, store, {
  atomic: change => registry.atomic(change),
  worldTools: agent => registry.tools(agent),
  owns: (agent, id) => tasks.get(id)?.agent === agent,
  session: (agent, line) => {
    const text = JSON.stringify({ type: "session_line", agent, line });
    for (const [socket, id] of watching) if (id === agent) socket.send(text);
  },
  log: (kind, fields) => log.write("agent", kind, fields),
  changed: () => publish("work"),
  readLog: (filter) => log.read(filter),
  holds: (agent, id) => {
    const task = tasks.get(id);
    return task?.state === "running" && task.agent === agent;
  },
  comment: async (id, text) => {
    const part = tasks.get(id)?.part;
    if (!part) throw new Error("no part was marked when this task was given: the user has to click or select one first");
    await comments(part, text);
  },
  published: ({ agent, task, type, text, title, changes }) => registry.atomic(() => {
    const id = world.next("update");
    tasks.change(task, { ...(title && { title }), latest: { id, type } });
    if (type === "finished" || type === "failed") tasks.end(task, type === "finished" ? "done" : "failed", text, changes);
    else tasks.change(task, { summary: text });
    world.add<Update>({ kind: "update", id, task, agent, type, text });
  }),
}, Math.max(0, ...tasks.all().map((task) => task.agent ?? 0)) + 1);

/** The pages, read from the folders at most every few seconds. */
let mapped: { at: number; pages: ReturnType<typeof map> } | null = null;
function listed() {
  if (!mapped || Date.now() - mapped.at > 5000) mapped = { at: Date.now(), pages: map(ROOT) };
  return mapped.pages;
}

/** Whether the service's code bundles: null when it does, or why not. Checked before a restart, so a broken service is never started. */
async function broken() {
  const out = join(tmpdir(), `live-build-${process.pid}`);
  try {
    const build = Bun.spawn([process.execPath, "build", "service.ts", "--target=bun", "--outdir", out], { cwd: import.meta.dir, stdout: "pipe", stderr: "pipe" });
    const [code, errors, output] = await Promise.all([build.exited, new Response(build.stderr).text(), new Response(build.stdout).text()]);
    if (code === 0) return null;
    return (errors || output).split("\n").map((line) => line.trim()).filter(Boolean).slice(0, 3).join(" ") || `bun build ended with ${code}`;
  } catch (error) {
    return error instanceof Error ? error.message : String(error);
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}

/** The page the user is on, as Fast is shown it. */
async function seen(): Promise<Seen | null> {
  if (!(owner ?? active)) return null;
  try {
    const read = await ask({ name: "read" });
    settle();
    if (typeof read?.address !== "string") return null;
    // While the canvas is open, the page answers with the pad on it.
    return { address: canvas?.pad ?? read.address, text: String(read.text ?? ""), marked: subject?.quote ?? "", panel: read.panel === "open", canvas: Boolean(canvas), pads: pads() };
  } catch (error) {
    log.write("service", "page_not_read", { text: error instanceof Error ? error.message : String(error) });
    return null;
  }
}

/** The first words of a text: a task's title until its agent names it. */
function start_of(text: string) {
  const words = text.replace(/\s+/g, " ").trim().split(" ");
  return words.length > 8 ? `${words.slice(0, 8).join(" ")}…` : words.join(" ");
}

/** The user typed a message in an agent's chat. It does not go through Fast: it is already addressed to one agent. It is a
    task of that agent like any other, so its updates reach the user the same way. */
function typed(id: number, text: string) {
  const said = text.trim();
  if (!said) return;
  if (!agents.has(id)) return log.write("service", "typed_to_no_agent", { agent: id, text: said });
  settle();
  const where = here()?.address ?? null;
  const part = subject;
  const prompt = `${said}${where ? `\n\nThe page the user is on: ${where}` : ""}${part ? `\nMarked on it: ${JSON.stringify(part.quote)}` : ""}`;
  const task = tasks.add({ title: start_of(said), request: prompt, page: where, part, agent: id });
  agents.send(id, { task: task.id, prompt });
  world.add({ kind: "task", id: task.id, agent: id, how: "send", from: null, prompt });
  log.write("user", "typed", { agent: id, task: task.id, text: said });
}

/** What the page answered an action with, or an error when it could not do it. */
async function page_does(action: Record<string, unknown>) {
  const done = await ask(action);
  if (done?.error) throw new Error(String(done.error));
  return done;
}

/** The hand-over a line is said for: the one the run answers, or the one that started the task an update it read is about. */
function handover_of({ handover, used }: Carried) {
  if (handover) return handover;
  for (const name of used) {
    if (!name.startsWith("u")) continue;
    const update = world.recent("update", 400).find((entry) => entry.id === Number(name.slice(1)));
    const origin = update && tasks.get(update.task)?.origin;
    if (origin && origin.session === log.session) return origin.handover;
  }
  return null;
}

/** The agents whose reports the line just given to Voice passes on, until Voice has said it. */
let passing: { agents: number[]; at: number } | null = null;
/** How long after a line was given what Voice says is taken to be that line. */
const SPOKEN_WITHIN = 30_000;

/** The agents whose answers a script read, when updates woke it and not words of the user's. */
function reported({ said, used }: Carried) {
  if (said.length > 0) return [];
  const updates = world.recent("update", 400);
  const read = used.filter((name) => name.startsWith("u")).map((name) => updates.find((entry) => entry.id === Number(name.slice(1))));
  return [...new Set(read.filter((entry) => entry && entry.type !== "progress").map((entry) => entry!.agent))];
}

/** Core commands keep their context-sensitive sandbox syntax and register their implementations once. */
function coreFunctions(): Record<CorePart, (as: Registering) => void> {
  return {
    session: (as) => {
      const { action, z } = as;
      action("say", { for: "fast", syntax: "say(text)", description: "Give Voice the line to speak.", input: { text: z.string() } }, (intent, { caller }) => {
        const carried = fastCaller(caller);
        if (!voice || voiceState !== "on") throw new Error("the voice is off");
        const handover = handover_of(carried);
        if (!voice.say(intent.text, handover, { run: carried.run })) throw new Error("the voice session is not connected");
        const agents = reported(carried);
        passing = agents.length > 0 ? { agents, at: Date.now() } : null;
        return { handover };
      });
      action("restart", { for: "fast", syntax: "restart(force)", description: "Restart the live session.", input: { force: z.boolean() } }, async (intent) => {
        const why = await broken();
        if (why) throw new Error(`the service's code does not build, so it was not restarted: ${why}`);
        const running = tasks.running();
        if (running.length > 0 && !intent.force) {
          throw new Error(`${running.length === 1 ? "one task is" : `${running.length} tasks are`} still running, and restarting interrupts ${running.length === 1 ? "it" : "them"}. Tell the user that, and that "restart now" does it anyway; use restart(true) only after the user said so`);
        }
        // The pages are told first, so each knows the close that follows is a restart, connects again at once and
        // starts the voice again when it was on. A few seconds are left for what is being said; then the process
        // ends, with what it started, and devpage starts it again on the next request, which is the page's.
        tell({ type: "restarting" });
        setTimeout(() => {
          log.write("service", "service_restarting", { text: `The service ends now: the user asked for a restart.${running.length > 0 ? ` ${running.length} running tasks are interrupted.` : ""}` });
          // The signal below reaches this process too: it is not a second ending.
          ending = true;
          try {
            process.kill(-process.pid, "SIGTERM");
          } catch {}
          process.exit(0);
        }, 6000);
        return { restarting: true, interrupted: running.length };
      });
    },
    work: (as) => {
      registerAgentFunctions(as, () => agents);
      const { action, z } = as;
      action("spawn", { for: "fast", syntax: "spawn(prompt, route)", description: "Give work to a new agent.", input: { prompt: z.string(), route: z.string().optional() } }, (intent, { caller }) => giveWork({ ...intent, do: "spawn" }, fastCaller(caller)));
      action("send", { for: "fast", syntax: "aN.send(prompt)", description: "Give work to an existing agent.", input: { agent: z.number(), prompt: z.string() } }, (intent, { caller }) => giveWork({ ...intent, do: "send" }, fastCaller(caller)));
      action("fork", { for: "fast", syntax: "aN.fork(prompt)", description: "Give work to a copy of an agent.", input: { agent: z.number(), prompt: z.string() } }, (intent, { caller }) => giveWork({ ...intent, do: "fork" }, fastCaller(caller)));
    },
    canvas: (as) => {
      const { action, z } = as;
      action("show", { for: "fast", syntax: "canvas.show(address)", description: "Show a scratch pad.", input: { address: z.string(), title: z.string().optional() } }, (intent) => registry.atomic(() => {
        showPad(intent.address, intent.title, "fast");
        return { canvas: intent.address };
      }));
      action("close", { for: "fast", syntax: "canvas.close()", description: "Close the canvas.", input: {} }, () => registry.atomic(() => {
        return { closed: closePad("fast") };
      }));
    },
    page: (as) => {
      const { action, z } = as;
      action("go", { for: "fast", syntax: "page.go(address)", description: "Open a listed page.", input: { address: z.string() } }, async (intent) => {
        // Going to a page means showing the page.
        closePad("fast");
        moving = Date.now() + 5000;
        const went = await page_does({ name: "go", address: intent.address });
        world.add({ kind: "page", by: "fast", text: moved({ ...intent, do: "go" }) });
        return went;
      });
      action("scroll", { for: "fast", syntax: "page.scroll(where)", description: "Scroll the page.", input: { where: z.enum(["up", "down", "top", "bottom"]) } }, async (intent) => {
        const done = await page_does({ name: "scroll", to: intent.where });
        world.add({ kind: "page", by: "fast", text: moved({ ...intent, do: "scroll" }) });
        return done;
      });
      action("panel", { for: "fast", syntax: "page.panel(open)", description: "Open or close the panel.", input: { open: z.boolean() } }, async (intent) => {
        const done = await page_does({ name: "panel", open: intent.open });
        world.add({ kind: "page", by: "fast", text: moved({ ...intent, do: "panel" }) });
        return done;
      });
      action("point", { for: "fast", syntax: "page.point(quote)", description: "Point at a quote on the page.", input: { quote: z.string() } }, async (intent) => {
        const done = await page_does({ name: "point", quote: intent.quote });
        world.add({ kind: "page", by: "fast", text: moved({ ...intent, do: "point" }) });
        return done;
      });
      action("select", { for: "fast", syntax: "page.select(quote)", description: "Select a quote on the page.", input: { quote: z.string() } }, async (intent) => {
        const found = await page_does({ name: "find", quote: intent.quote });
        if (!found.part) throw new Error("the page has no such text");
        registry.atomic(() => {
          mark(found.part, "assistant");
          world.add({ kind: "page", by: "fast", text: moved({ ...intent, do: "select" }) });
        });
        return { marked: true };
      });
    },
  };
}

function fastCaller(caller: import("./registry.ts").Caller): Carried {
  if (caller.who !== "fast") throw new Error("Only Fast can run this command");
  return { ...caller, said: caller.said ?? [] };
}

function giveWork(intent: Extract<Intent, { do: "spawn" | "send" | "fork" }>, carried: Carried) {
  return registry.atomic(() => {
    if (intent.do !== "spawn" && !agents.has(intent.agent)) throw new Error(`a${intent.agent} no longer exists`);
    // Fast decides the route, in its script. Here it is only checked: a name that is not an enabled route is refused,
    // and Fast is told, so that the decision stays its own. No name is the default route, which Fast was told too.
    const route = intent.do === "spawn" && intent.route ? find(routes, intent.route) : undefined;
    if (intent.do === "spawn" && intent.route && !route) throw new Error(`${JSON.stringify(intent.route)} is not a route. The routes are: ${routes.map((one) => one.name).join(", ")}`);
    // The message the work comes from: the one of this hand-over that the script put in the prompt.
    const from = carried.said.find((message) => carried.used.includes(`m${message.id}`)) ?? carried.said.at(-1) ?? null;
    const part = from?.part ?? null;
    const task = tasks.add({ title: start_of(from?.text ?? intent.prompt), request: intent.prompt, page: from?.page ?? here()?.address ?? null, part, origin: carried.handover ? { session: log.session, handover: carried.handover } : undefined });
    const job = { task: task.id, prompt: intent.prompt };
    const agent = intent.do === "spawn" ? agents.spawn(job, route ?? routes[0]) : intent.do === "fork" ? agents.fork(intent.agent, job) : agents.send(intent.agent, job);
    tasks.change(task.id, { agent });
    world.add({ kind: "task", id: task.id, agent, how: intent.do, from: intent.do === "fork" ? intent.agent : null, prompt: intent.prompt });
    // The marked part went with the task: the next command starts with nothing marked.
    if (part && JSON.stringify(subject) === JSON.stringify(part)) {
      subject = null;
      publish("page");
    }
    return { task: task.id, agent, ...(intent.do === "spawn" ? { route: (route ?? routes[0]).name } : {}), from_message: from?.id ?? null, marked: part?.quote ?? null };
  });
}

const commandPart = {
  say: "session", restart: "session",
  spawn: "work", send: "work", fork: "work",
  show: "canvas", close: "canvas",
  go: "page", scroll: "page", panel: "page", point: "page", select: "page",
} as const;

function carry(intent: Intent, carried: Carried) {
  const caller = { who: "fast" as const, ...carried };
  if (intent.do === "world") return registry.call(intent.name, intent.input, caller);
  const { do: command, ...input } = intent;
  return registry.call(`${commandPart[command]}.${command}`, input, caller);
}

publish("work", "page", "canvas", "session");
const fast: Fast | null = PROVIDER
  ? new Fast(PROVIDER, FAST_MODEL, {
      world, registry,
      page: seen,
      pages: listed,
      // The rules are read again each time Fast's context is built, so a change of `routing.md` needs no restart.
      // The project's own replace the ones for every project.
      routes: () => told(routes, rules(ROOT, HOME)),
      work: () => registry.state("work") as unknown as import("./fast.ts").WorkState,
      carry,
      forget: (keep) => void agents.forget(keep),
      quiet: () => Date.now() - spoke > 1500,
      log: (kind, fields) => log.write("fast", kind, fields),
    })
  : null;

/** The user's messages that no hand-over has come for yet. */
let loose: Message[] = [];

/** A hand-over without words of its own: it is for the user's messages that wait for one. */
function handed(handover: string) {
  if (loose.length === 0) return false;
  fast?.wake("handover", { id: handover, messages: loose });
  loose = [];
  return true;
}

/** The user or Voice said something: it becomes a message of the world, with where the user was and what the user had marked. A hand-over wakes Fast. */
function said({ who, text, start_ms, end_ms, handover }: Said) {
  settle();
  // What the user said goes into the transcript beside the recording of their voice.
  if (who === "me") recording?.said({ start_ms, end_ms, text });
  const part = who === "me" ? subject : null;
  const about = who === "voice" && passing && Date.now() - passing.at < SPOKEN_WITHIN ? passing.agents : undefined;
  if (who === "voice") passing = null;
  const message = world.add<Message>({ kind: "message", id: world.next("message"), who, text, page: here()?.address ?? null, marked: part?.quote ?? "", ...(part ? { part } : {}), start_ms, end_ms, handover, ...(about ? { about } : {}) });
  if (who !== "me") return;
  loose.push(message);
  if (handover) handed(handover);
}

function snapshot(part: CorePart): Record<string, unknown> {
  switch (part) {
    case "work": return { tasks: tasks.all(), agents: agents.all(), chats: true };
    case "session": return { voice: voiceState, problem };
    case "page": return { address: known, subject, by: subject ? author : null };
    case "canvas": return { canvas };
  }
}

function publish(...parts: CorePart[]) {
  for (const part of parts) registry.commit(part, snapshot(part), []);
}

function tell(message: unknown) {
  const text = JSON.stringify(message);
  for (const socket of sockets) socket.send(text);
}

/** A request to devpage itself: its comments. */
async function devpage(path: string, body: unknown) {
  const response = await fetch(DEVPAGE + path, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  if (!response.ok) throw new Error(`devpage answered ${response.status}: ${await response.text()}`);
  return response.json().catch(() => ({}));
}

/** Asks the user's open page to do something, and waits for what it answers. */
const waiting = new Map<number, (value: any) => void>();
let asked = 0;
function ask(action: Record<string, unknown>): Promise<any> {
  const to = owner ?? active;
  if (!to) return Promise.reject(new Error("no page is open"));
  const id = ++asked;
  to.send(JSON.stringify({ type: "action", id, ...action }));
  return new Promise((answered, failed) => {
    waiting.set(id, answered);
    setTimeout(() => waiting.delete(id) && failed(new Error("the page did not answer")), 8000);
  });
}

/** Writes a comment on a part, when the user asked for one; returns the thread's id. */
async function comments(part: Part, text: string) {
  const { page, quote, prefix = "", suffix = "" } = part;
  const made = await devpage("/__devpage/comments", { page, anchor: { quote, prefix, suffix }, text });
  return made.id as string;
}

function mark(part: Part | null, by: "user" | "assistant") {
  // A part of some other page than the one the user is on is never the subject.
  if (part && !shows(here(), part)) part = null;
  subject = part;
  author = by;
  publish("page");
}

/** What the page measured of a start: when the user pressed, by the clock this service shares with it, and how long its microphone and audio took. */
interface Timing {
  pressed: number;
  microphone_ms: number;
  audio_ms: number;
  kept: boolean;
}

/** A start's timing as the page sent it; null from a page that sends none, or nonsense. */
function timingOf(sent: any): Timing | null {
  if (!sent || !Number.isFinite(sent.pressed)) return null;
  return { pressed: sent.pressed, microphone_ms: Number(sent.microphone_ms) || 0, audio_ms: Number(sent.audio_ms) || 0, kept: sent.kept === true };
}

/** The voice came on and no audio of the user's has reached it yet: when it came on, and when the user pressed. */
let ready: { at: number; pressed: number } | null = null;

/** Starts the voice for a tab. `again` when the page does it by itself, because the voice was on when the service ended or the page was loaded again. */
async function start(socket: Socket, again = false, timing: Timing | null = null) {
  const asked = Date.now();
  // The page's part of the start: from the press to this request, and how long its microphone and audio took to open.
  const page = timing ? { press_to_request_ms: asked - timing.pressed, microphone_ms: timing.microphone_ms, audio_ms: timing.audio_ms, audio_kept: timing.kept } : {};
  const pressed = timing?.pressed ?? asked;
  if (voiceState !== "off") return void log.write("service", "voice_start_ignored", { text: `A start was asked for while the voice was ${voiceState}.`, resumed: again, ...page });
  if (!PROVIDER) {
    problem = `${NEEDS} is not set in devpage's environment`;
    log.write("service", "voice_start_failed", { text: problem, resumed: again, ...page });
    return publish("session");
  }
  voiceState = "starting";
  problem = null;
  owner = socket;
  // A new session starts with nothing marked.
  if (!again) subject = null;
  publish("session", "page");
  const id = randomUUID();
  log.session = id;
  log.write("service", "voice_start_asked", { resumed: again, ...page });
  /** The session ended or never started: the voice is off, and the user is shown why when it was not the user's doing. */
  const off = (why: string | null) => {
    if (voice !== session) return;
    voice = null;
    recording?.close();
    recording = null;
    fast?.listen(false);
    owner = null;
    voiceState = "off";
    problem = why;
    subject = null;
    publish("page", "session");
  };
  const session: Voice = new Voice(PROVIDER, {
    audio: (pcm) => owner?.send(pcm),
    log: (who, kind, fields) => log.write(who, kind, fields),
    said,
    handed,
    heard: (who, text) => {
      const now = Date.now();
      if (who === "me") spoke = now;
      heard = heard.who === who && now - heard.at < 4000 ? { who, text: heard.text + text, at: now } : { who, text, at: now };
      tell({ type: "heard", who: heard.who, text: heard.text });
    },
    closed: (why) => {
      if (log.session === id) {
        log.write("service", "voice_session_ended", { text: why ?? "The user stopped it." });
        log.session = null;
      }
      off(why);
    },
  });
  voice = session;
  try {
    await session.started;
  } catch (error) {
    const why = error instanceof Error ? error.message : String(error);
    if (log.session === id) {
      log.write("service", "voice_session_failed", { text: why, request_to_failure_ms: Date.now() - asked, press_to_failure_ms: Date.now() - pressed });
      log.session = null;
    }
    return off(why);
  }
  if (voice !== session) return;
  voiceState = "on";
  ready = { at: Date.now(), pressed };
  log.write("service", "voice_ready", { resumed: again, request_to_ready_ms: ready.at - asked, press_to_ready_ms: ready.at - pressed });
  if (RECORD) {
    recording = new Recording(join(STATE, "voice"), id);
    log.write("service", "recording_started", { text: recording.path });
  }
  known = here()?.address ?? null;
  // The user did nothing to get the voice back, so it says that it is.
  if (again) voice.say("I'm back.", null, { why: "the voice came back by itself" });
  // Fast gets what the agents published while no voice was on, and decides what of it the user hears now.
  fast?.listen(true);
  publish("session", "page");
}

function stop() {
  voice?.stop();
}

function on(socket: Socket, message: any) {
  switch (message.type) {
    case "start":
      active = socket;
      return void start(socket, message.resumed === true, timingOf(message.timing));
    case "start_failed":
      // The page gave up before the service had a session: the microphone did not open, or the request was never answered.
      return void log.write("service", "voice_start_failed", { text: String(message.text).slice(0, 300), press_to_failure_ms: Number(message.ms) || null });
    case "stop":
      active = socket;
      return stop();
    case "page":
      // A tab in the background reports too, when a file changes under it: only the tab the user is in becomes the one
      // the user used last.
      pages.set(socket, message.page);
      if (message.focused || !active) active = socket;
      return registry.atomic(settle);
    case "subject":
      active = socket;
      settle();
      // Only the user's own click or selection arrives here: a script's `page.select` marks the part in the service.
      return registry.atomic(() => {
        world.add({ kind: "page", by: "me", text: message.part ? `The user marked ${JSON.stringify(message.part.quote)}.` : "The user cleared the mark." });
        mark(message.part, "user");
      });
    case "canvas":
      // The user pressed the cross, or clicked a task whose change is on a pad.
      active = socket;
      try {
        registry.atomic(() => message.open ? showPad(String(message.address), undefined, "me") : closePad("me"));
      } catch (error) {
        log.write("service", "canvas_not_shown", { text: error instanceof Error ? error.message : String(error) });
      }
      return;
    case "agent":
      // The user opened an agent's chat, or went back to the list: the page gets that agent's session, and then what is added.
      if (typeof message.id !== "number") return void watching.delete(socket);
      watching.set(socket, message.id);
      // Read from what the harness keeps of the conversation: nothing of it is kept here.
      return void agents.session(message.id).then(
        (turns) => watching.get(socket) === message.id && socket.send(JSON.stringify({ type: "session", agent: message.id, turns })),
        (error) => log.write("service", "session_not_read", { agent: message.id, text: error instanceof Error ? error.message : String(error) }),
      );
    case "message":
      active = socket;
      return registry.atomic(() => typed(Number(message.agent), String(message.text ?? "")));
    case "call":
      // A widget calls a function of the world: what it wrote reaches every page as its part's state.
      return void registry.call(String(message.name), message.input ?? {}, { who: "page" }).catch((error) => log.write("service", "call_failed", { text: `${message.name}: ${error instanceof Error ? error.message : String(error)}` }));
    case "result":
      waiting.get(message.id)?.(message.value);
      waiting.delete(message.id);
      return;
  }
}

Bun.serve<undefined, never>({
  hostname: "127.0.0.1",
  port: PORT,
  fetch(request, server) {
    // Only devpage knows the token: anything else on this machine that finds the port is refused.
    if (request.headers.get("x-devpage-token") !== TOKEN) return new Response("not devpage", { status: 403 });
    const { pathname } = new URL(request.url);
    if (pathname === "/socket") return server.upgrade(request) ? undefined : new Response("expected a WebSocket", { status: 400 });
    if (pathname === "/state") return Response.json(Object.fromEntries(registry.parts().map(part => [part, registry.state(part)])));
    // A scratch pad's Markdown: devpage draws it as a page, for the canvas.
    if (pathname.startsWith("/pages/")) {
      const address = decodeURIComponent(pathname.slice("/pages/".length));
      if (!PAD.test(address) || !existsSync(padFile(address))) return new Response("not found", { status: 404 });
      return new Response(readFileSync(padFile(address), "utf8"), { headers: { "content-type": "text/markdown; charset=utf-8" } });
    }
    // An agent's session: its turns and their items, in the shape every harness is turned into.
    const session = /^\/session\/(\d+)$/.exec(pathname);
    if (session) return agents.session(Number(session[1])).then((turns) => Response.json({ agent: Number(session[1]), turns }));
    return new Response("not found", { status: 404 });
  },
  websocket: {
    open(socket) {
      sockets.add(socket);
      for (const part of registry.parts()) socket.send(JSON.stringify({ type: "state", part, state: registry.state(part) }));
    },
    message(socket, message) {
      if (typeof message === "string") return on(socket, JSON.parse(message));
      // The user's voice. Only the tab that started the session is heard.
      if (socket !== owner || !voice || voiceState !== "on") return;
      if (ready) {
        // The first piece of the user's audio that reaches the voice model: what came before it was never sent.
        log.write("service", "voice_first_audio", { ready_to_audio_ms: Date.now() - ready.at, press_to_audio_ms: Date.now() - ready.pressed });
        ready = null;
      }
      voice.hear(message);
      recording?.add(message);
    },
    close(socket) {
      sockets.delete(socket);
      pages.delete(socket);
      watching.delete(socket);
      if (active === socket) active = null;
      // The tab with the microphone is gone: the voice stops with it.
      if (owner === socket) stop();
      settle();
    },
  },
});

// Ended from outside, by devpage or by hand: the pages are told first, as before a restart, so that they come back
// by themselves. A moment is left for the message to go out.
let ending = false;
for (const signal of ["SIGTERM", "SIGINT"] as const) {
  process.on(signal, () => {
    if (ending) return;
    ending = true;
    log.write("service", "service_ending", { text: `The service ends now: it got ${signal}.` });
    tell({ type: "restarting" });
    setTimeout(() => process.exit(0), 150);
  });
}
