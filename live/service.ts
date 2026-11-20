import { SessionStore } from "./store.ts";
// The live session's service. devpage starts it and passes it every request and WebSocket under the extension's
// `live/` address. It holds the voice session, Fast, the slow agents, the world and the tasks, and keeps the keys:
// none reaches the browser. Voice hands everything the user says to Fast; Fast answers with a script, and this file carries
// out what the script asked for: a line for Voice, work for an agent, a call of a part's function. What the agents publish goes
// into the world, and Fast decides what the user hears of it.
import { randomUUID } from "node:crypto";
import { rmSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { isAbsolute, join } from "node:path";
import type { ServerWebSocket } from "bun";
import { Agents, agentFunctions } from "./agents.ts";
import { ClaudeCode } from "./claude.ts";
import { Codex } from "./codex.ts";
import type { Harness } from "./harness.ts";
import { checked, enabledRoutes, find, rules, settings, told, type Enabled, type Route } from "./routes.ts";
import { NEEDS, provider } from "./provider.ts";
import { Recording } from "./recording.ts";
import { coreEntry } from "./events.ts";
import { registerCore, type CoreCall, type CorePart } from "./core-parts.ts";
import { Registry, type Caller, type Now } from "./registry.ts";
import { BACKEND, Fast, type Carried } from "./fast.ts";
import { devpage as devpagePart } from "./parts/devpage/part.ts";
import { canvas as canvasPart } from "./parts/canvas/part.ts";
import { everywhere, here as project, HOME, stateOf } from "./home.ts";
import { map } from "./pages.ts";
import { Log } from "./log.ts";
import type { Intent } from "./sandbox.ts";
import { Tasks, type Task, type Part } from "./tasks.ts";
import { Voice, type Said } from "./voice.ts";
import { World, type Message, type Update } from "./world.ts";

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
/** The last thing heard or said, as it grows. */
let heard = { who: "me" as "me" | "ai", text: "", at: 0 };
/** When the user was last heard speaking. */
let spoke = 0;

/** Where the scratch pads are: Markdown files that are not in the project, one per idea. */
const PADS = join(STATE, "pads");
mkdirSync(PADS, { recursive: true });

const priorTasks = registry.state("work").tasks as Task[] | undefined;
const tasks = new Tasks(priorTasks ?? store.get<Task[]>("tasks") ?? [], () => publish("work"));

/** The world's registered parts, with their state and entries in one store. A page is sent a part's state when it changes. */
registerCore(registry, carryCore, { work: agentFunctions(() => agents) });
/** The two parts the program brings: the user's page, and the canvas over it. */
const userPage = devpagePart(registry, {
  ask,
  tab: () => {
    const tab = owner ?? active;
    return (tab && pages.get(tab)) ?? null;
  },
  pages: listed,
  comment: comments,
  marked: (agent, id) => {
    const task = tasks.get(id);
    if (task?.agent !== agent) throw new Error(`Task ${id} is not yours.`);
    return task.part;
  },
});
const canvas = canvasPart(registry, userPage, { folder: PADS, log: (kind, text) => log.write("service", kind, { text }) });
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
  const core = coreEntry(entry);
  const work = core?.kind === "task" || core?.kind === "update" ? { agent: core.agent, task: core.kind === "task" ? core.id : core.task } : {};
  log.write("world", entry.kind, { part: entry.part, text: registry.line(entry), n: entry.n, ...work });
  if (wakes) fast?.entry(core ? core.kind : `${entry.part}.${entry.kind}`, entry.n);
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
  texts: () => registry.texts(),
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
  published: ({ agent, task, type, text, title, changes }) => registry.atomic(() => {
    const id = world.next("update");
    tasks.change(task, { ...(title && { title }), latest: { id, type } });
    if (type === "finished" || type === "failed") tasks.end(task, type === "finished" ? "done" : "failed", text, changes);
    else tasks.change(task, { summary: text });
    world.add<Update>({ kind: "update", id, task, agent, type, text });
  }),
}, Math.max(0, ...tasks.all().map((task) => task.agent ?? 0)) + 1, SETTINGS.fullFilesystemAccess === true);

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
  const { page, part } = userPage.stamp();
  const task = tasks.add({ title: start_of(said), request: said, page, part, agent: id });
  agents.send(id, { task: task.id, prompt: given(said, page, part?.quote ?? "") });
  world.add({ kind: "task", id: task.id, agent: id, how: "send", from: null, prompt: said });
  log.write("user", "typed", { agent: id, task: task.id, text: said });
}

/** A task as its agent gets it: what was asked, and after it what each part adds, such as the page the user is on. */
function given(prompt: string, page: string | null, marked: string) {
  return [prompt, ...registry.added({ prompt, page, marked })].join("\n\n");
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

/** Carries out one of Fast's own calls: a line for Voice, a restart, work for an agent. */
function carryCore(call: CoreCall, input: any, { caller }: Now) {
  switch (call) {
    case "say": {
      const carried = fastCaller(caller);
      if (!voice || voiceState !== "on") throw new Error("the voice is off");
      const handover = handover_of(carried);
      if (!voice.say(input.text, handover, { run: carried.run })) throw new Error("the voice session is not connected");
      const agents = reported(carried);
      passing = agents.length > 0 ? { agents, at: Date.now() } : null;
      return { handover };
    }
    case "restart":
      return restart(input.force);
    case "spawn":
    case "send":
    case "fork":
      return giveWork({ ...input, do: call }, fastCaller(caller));
  }
}

/** Restarts the service, when its code builds and no task is running or the user said to do it anyway. */
async function restart(force: boolean) {
  const why = await broken();
  if (why) throw new Error(`the service's code does not build, so it was not restarted: ${why}`);
  const running = tasks.running();
  if (running.length > 0 && !force) {
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

function fastCaller(caller: Caller): Carried {
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
    const page = from ? from.page : userPage.stamp().page;
    const task = tasks.add({ title: start_of(from?.text ?? intent.prompt), request: intent.prompt, page, part, origin: carried.handover ? { session: log.session, handover: carried.handover } : undefined });
    const job = { task: task.id, prompt: given(intent.prompt, page, from?.marked ?? "") };
    const agent = intent.do === "spawn" ? agents.spawn(job, route ?? routes[0]) : intent.do === "fork" ? agents.fork(intent.agent, job) : agents.send(intent.agent, job);
    tasks.change(task.id, { agent });
    world.add({ kind: "task", id: task.id, agent, how: intent.do, from: intent.do === "fork" ? intent.agent : null, prompt: intent.prompt });
    // The marked part went with the task: the next command starts with nothing marked.
    if (part) userPage.gave(part);
    return { task: task.id, agent, ...(intent.do === "spawn" ? { route: (route ?? routes[0]).name } : {}), from_message: from?.id ?? null, marked: part?.quote ?? null };
  });
}

registry.atomic(() => {
  publish("work", "session");
  store.db.run("delete from session_state where name = 'tasks'");
});
const fast: Fast | null = PROVIDER
  ? new Fast(PROVIDER, FAST_MODEL, {
      world, registry,
      // The rules are read again each time Fast's context is built, so a change of `routing.md` needs no restart.
      // The project's own replace the ones for every project.
      routes: () => told(routes, rules(ROOT, HOME)),
      routeNames: () => routes.map(route => route.name),
      work: () => registry.state("work") as unknown as import("./fast.ts").WorkState,
      carry: (intent, carried) => {
        const { name, input } = registry.requested(intent);
        return registry.call(name, input, { who: "fast", ...carried });
      },
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
  const now = userPage.stamp();
  // What the user said goes into the transcript beside the recording of their voice.
  if (who === "me") recording?.said({ start_ms, end_ms, text });
  const part = who === "me" ? now.part : null;
  const about = who === "voice" && passing && Date.now() - passing.at < SPOKEN_WITHIN ? passing.agents : undefined;
  if (who === "voice") passing = null;
  const message = world.add<Message>({ kind: "message", id: world.next("message"), who, text, page: now.page, marked: part?.quote ?? "", ...(part ? { part } : {}), start_ms, end_ms, handover, ...(about ? { about } : {}) });
  if (who !== "me") return;
  loose.push(message);
  if (handover) handed(handover);
}

function snapshot(part: CorePart): Record<string, unknown> {
  switch (part) {
    case "work": return { tasks: tasks.all(), agents: agents.all(), chats: true };
    case "session": return { voice: voiceState, problem };
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
  if (!again) userPage.clear();
  publish("session");
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
    userPage.clear();
    publish("session");
  };
  const session: Voice = new Voice(PROVIDER, {
    texts: () => registry.texts(),
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
  userPage.arrive();
  // The user did nothing to get the voice back, so it says that it is.
  if (again) voice.say("I'm back.", null, { why: "the voice came back by itself" });
  // Fast gets what the agents published while no voice was on, and decides what of it the user hears now.
  fast?.listen(true);
  publish("session");
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
      return registry.atomic(userPage.settle);
    case "subject":
      active = socket;
      // Only the user's own click or selection arrives here: a script's `devpage.select` marks the part in the service.
      return registry.atomic(() => userPage.marked(message.part ?? null));
    case "canvas":
      // The user pressed the cross, or clicked a task whose change is on a pad.
      active = socket;
      try {
        registry.atomic(() => message.open ? canvas.opened(String(message.address)) : canvas.closed());
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
      const markdown = canvas.markdown(address);
      if (markdown === null) return new Response("not found", { status: 404 });
      return new Response(markdown, { headers: { "content-type": "text/markdown; charset=utf-8" } });
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
      registry.atomic(userPage.settle);
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
