// The live session's service. devpage starts it and passes it every request and WebSocket under the extension's
// `live/` address. It holds the voice session, Fast, the slow agents, the world and the tasks, and keeps the keys:
// none reaches the browser. Voice hands everything the user says to Fast; Fast answers with a script, and this file carries
// out what the script asked for: a line for Voice, work for an agent, a move of the user's page. What the agents publish goes
// into the world, and Fast decides what the user hears of it.
import { randomUUID } from "node:crypto";
import { existsSync, readFileSync, readdirSync, rmSync, mkdirSync, statSync, watch, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { ServerWebSocket } from "bun";
import { Agents } from "./agents.ts";
import { ClaudeCode } from "./claude.ts";
import { Codex } from "./codex.ts";
import type { Harness } from "./harness.ts";
import { checked, enabledRoutes, find, rules, settings, told, type Enabled, type Route } from "./routes.ts";
import { NEEDS, provider } from "./provider.ts";
import { Recording } from "./recording.ts";
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
/** Who said and did what, exactly: one file per day. */
const log = new Log(join(STATE, "log"));
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
let known: string | null = null;
/** What is marked, by the user or by a script's `page.select`: always a part of the page the user is on. */
let subject: Part | null = null;
/** Who marked it: the page draws the user's mark in the colour of the user's voice and the assistant's in its own. Means nothing while nothing is marked. */
let author: "user" | "assistant" = "user";
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
let canvas: { pad: string; version: number } | null = null;

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
  tell();
}

/** Closes the canvas: the page under it is the page the user is on again. */
function closePad(by: "me" | "fast") {
  if (!canvas) return false;
  canvas = null;
  subject = null;
  known = here()?.address ?? null;
  world.add({ kind: "page", by, text: by === "fast" ? "Fast closed the canvas." : "The user closed the canvas themselves." });
  tell();
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
      if (!existsSync(padFile(canvas.pad))) return void closePad("fast");
      canvas = { ...canvas, version: Date.now() };
      tell();
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
  }
  if (subject && !shows(page, subject)) {
    subject = null;
    tell();
  }
}

const tasks = new Tasks(join(STATE, "tasks.json"), () => tell());

/** The world. Every entry also goes into the log, as the line Fast reads; an update or a problem wakes Fast. */
const world: World = new World(join(STATE, "world.jsonl"), (entry) => {
  log.write("world", entry.kind, { text: line(entry), n: entry.n, ...(entry.kind === "update" || entry.kind === "task" ? { agent: entry.agent, task: entry.kind === "task" ? entry.id : entry.task } : {}) });
  if (entry.kind === "update" || entry.kind === "problem") fast?.wake(entry.kind);
});

/** The part that was marked when each of the user's messages was said: a task given from the message is about it. */
const marks = new Map<number, Part>();
/** The hand-over each task was given in, with its voice session: news of the task is said as that hand-over's. */
const origins = new Map<number, { session: string | null; handover: string }>();

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

const agents = new Agents(ROOT, PADS, enabled, routes, join(STATE, "agents.json"), {
  owns: (agent, id) => tasks.get(id)?.agent === agent,
  session: (agent, line) => {
    const text = JSON.stringify({ type: "session_line", agent, line });
    for (const [socket, id] of watching) if (id === agent) socket.send(text);
  },
  log: (kind, fields) => log.write("agent", kind, fields),
  changed: () => tell(),
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
  published: ({ agent, task, type, text, title, changes }) => {
    if (title) tasks.change(task, { title });
    if (type === "finished" || type === "failed") tasks.end(task, type === "finished" ? "done" : "failed", text, changes);
    else tasks.change(task, { summary: text });
    world.add<Update>({ kind: "update", id: world.next("update"), task, agent, type, text });
  },
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
    const origin = update && origins.get(update.task);
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

/** Carries out one thing a script of Fast's asked for. Throws when it cannot be done: Fast is told. */
async function carry(intent: Intent, carried: Carried): Promise<unknown> {
  switch (intent.do) {
    case "say": {
      if (!voice || voiceState !== "on") throw new Error("the voice is off");
      const handover = handover_of(carried);
      if (!voice.say(intent.text, handover, { run: carried.run })) throw new Error("the voice session is not connected");
      const agents = reported(carried);
      passing = agents.length > 0 ? { agents, at: Date.now() } : null;
      return { handover };
    }
    case "spawn":
    case "send":
    case "fork": {
      if (intent.do !== "spawn" && !agents.has(intent.agent)) throw new Error(`a${intent.agent} no longer exists`);
      // Fast decides the route, in its script. Here it is only checked: a name that is not an enabled route is refused,
      // and Fast is told, so that the decision stays its own. No name is the default route, which Fast was told too.
      const route = intent.do === "spawn" && intent.route ? find(routes, intent.route) : undefined;
      if (intent.do === "spawn" && intent.route && !route) throw new Error(`${JSON.stringify(intent.route)} is not a route. The routes are: ${routes.map((one) => one.name).join(", ")}`);
      // The message the work comes from: the one of this hand-over that the script put in the prompt.
      const from = carried.said.find((message) => carried.used.includes(`m${message.id}`)) ?? carried.said.at(-1) ?? null;
      const part = (from && marks.get(from.id)) ?? null;
      const task = tasks.add({ title: start_of(from?.text ?? intent.prompt), request: intent.prompt, page: from?.page ?? here()?.address ?? null, part });
      const job = { task: task.id, prompt: intent.prompt };
      const agent = intent.do === "spawn" ? agents.spawn(job, route ?? routes[0]) : intent.do === "fork" ? agents.fork(intent.agent, job) : agents.send(intent.agent, job);
      tasks.change(task.id, { agent });
      if (carried.handover) origins.set(task.id, { session: log.session, handover: carried.handover });
      world.add({ kind: "task", id: task.id, agent, how: intent.do, from: intent.do === "fork" ? intent.agent : null, prompt: intent.prompt });
      // The marked part went with the task: the next command starts with nothing marked.
      if (part && subject === part) {
        subject = null;
        tell();
      }
      return { task: task.id, agent, ...(intent.do === "spawn" ? { route: (route ?? routes[0]).name } : {}), from_message: from?.id ?? null, marked: part?.quote ?? null };
    }
    case "show": {
      showPad(intent.address, intent.title, "fast");
      return { canvas: intent.address };
    }
    case "close": {
      return { closed: closePad("fast") };
    }
    case "go": {
      // Going to a page means showing the page.
      closePad("fast");
      moving = Date.now() + 5000;
      const went = await page_does({ name: "go", address: intent.address });
      world.add({ kind: "page", by: "fast", text: moved(intent) });
      return went;
    }
    case "scroll": {
      const done = await page_does({ name: "scroll", to: intent.where });
      world.add({ kind: "page", by: "fast", text: moved(intent) });
      return done;
    }
    case "panel": {
      const done = await page_does({ name: "panel", open: intent.open });
      world.add({ kind: "page", by: "fast", text: moved(intent) });
      return done;
    }
    case "point": {
      const done = await page_does({ name: "point", quote: intent.quote });
      world.add({ kind: "page", by: "fast", text: moved(intent) });
      return done;
    }
    case "select": {
      const found = await page_does({ name: "find", quote: intent.quote });
      if (!found.part) throw new Error("the page has no such text");
      mark(found.part, "assistant");
      world.add({ kind: "page", by: "fast", text: moved(intent) });
      return { marked: true };
    }
    case "restart": {
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
    }
  }
}

const fast: Fast | null = PROVIDER
  ? new Fast(PROVIDER, FAST_MODEL, {
      world,
      page: seen,
      pages: listed,
      // The rules are read again each time Fast's context is built, so a change of `routing.md` needs no restart.
      // The project's own replace the ones for every project.
      routes: () => told(routes, rules(ROOT, HOME)),
      tasks: () => tasks.all(),
      agent: (id) => agents.has(id),
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
  const message = world.add<Message>({ kind: "message", id: world.next("message"), who, text, page: here()?.address ?? null, marked: part?.quote ?? "", start_ms, end_ms, handover, ...(about ? { about } : {}) });
  if (part) marks.set(message.id, part);
  if (who !== "me") return;
  loose.push(message);
  if (handover) handed(handover);
}

function state() {
  return { type: "state", voice: voiceState, problem, tasks: tasks.all(), agents: agents.all().map(({ id, name, state }) => ({ id, name, state })), chats: true, subject, by: subject ? author : null, canvas };
}

/** Sends every open page the state, or one message. */
function tell(message: unknown = state()) {
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
  tell();
}

/** Starts the voice for a tab. `again` when the page does it by itself, because the voice was on when the service ended or the page was loaded again. */
async function start(socket: Socket, again = false) {
  if (voiceState !== "off") return;
  if (!PROVIDER) {
    problem = `${NEEDS} is not set in devpage's environment`;
    return tell();
  }
  voiceState = "starting";
  problem = null;
  owner = socket;
  // A new session starts with nothing marked.
  subject = null;
  tell();
  const id = randomUUID();
  log.session = id;
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
    tell();
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
      log.write("service", "voice_session_failed", { text: why });
      log.session = null;
    }
    return off(why);
  }
  if (voice !== session) return;
  voiceState = "on";
  if (RECORD) {
    recording = new Recording(join(STATE, "voice"), id);
    log.write("service", "recording_started", { text: recording.path });
  }
  known = here()?.address ?? null;
  // The user did nothing to get the voice back, so it says that it is.
  if (again) voice.say("I'm back.", null, { why: "the voice came back by itself" });
  // Fast gets what the agents published while no voice was on, and decides what of it the user hears now.
  fast?.listen(true);
  tell();
}

function stop() {
  voice?.stop();
}

function on(socket: Socket, message: any) {
  switch (message.type) {
    case "start":
      active = socket;
      return void start(socket, message.resumed === true);
    case "stop":
      active = socket;
      return stop();
    case "page":
      // A tab in the background reports too, when a file changes under it: only the tab the user is in becomes the one
      // the user used last.
      pages.set(socket, message.page);
      if (message.focused || !active) active = socket;
      return settle();
    case "subject":
      active = socket;
      settle();
      // Only the user's own click or selection arrives here: a script's `page.select` marks the part in the service.
      world.add({ kind: "page", by: "me", text: message.part ? `The user marked ${JSON.stringify(message.part.quote)}.` : "The user cleared the mark." });
      return mark(message.part, "user");
    case "canvas":
      // The user pressed the cross, or clicked a task whose change is on a pad.
      active = socket;
      try {
        if (message.open) showPad(String(message.address), undefined, "me");
        else closePad("me");
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
      return typed(Number(message.agent), String(message.text ?? ""));
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
    if (pathname === "/state") return Response.json(state());
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
      socket.send(JSON.stringify(state()));
    },
    message(socket, message) {
      if (typeof message === "string") return on(socket, JSON.parse(message));
      // The user's voice. Only the tab that started the session is heard.
      if (socket !== owner || !voice || voiceState !== "on") return;
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
