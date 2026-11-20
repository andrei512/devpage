// The live session, as the page sees it: one connection to the session's service, the user's microphone going up and the
// AI's voice coming down while the session is on, and the page told what to mark and where to go. The widgets in
// devpage's places — the orb, the tasks button, the tasks list — all read this one module.
//
// A restart of the service is not the user's to repair: the page connects again by itself, and a voice that was on starts
// again by itself, with the microphone and the sound kept open in between.
import { useSyncExternalStore } from "react";
import { base, page, useWorld, world as parts } from "devpage";

export interface Part {
  page: string;
  quote: string;
  prefix?: string;
  suffix?: string;
}

export interface Task {
  id: number;
  title: string;
  request: string;
  state: "running" | "done" | "failed";
  page: string | null;
  part: Part | null;
  summary: string | null;
  changes: Part[];
  /** The agent that has it. Left out for a task from before the agents. */
  agent?: number;
  started?: number;
  ended?: number | null;
}

/** A slow agent, as the side panel shows it. A closed agent is not sent. */
export interface Agent {
  /** What it is, for good: its orb is drawn from this number. */
  id: number;
  /** What it works on, as it named itself. None until its first update. */
  name: string | null;
  /** "asked": it has a question for the user. */
  state: "working" | "asked" | "idle";
}

/** One thing that happened in a turn of an agent: the same for every harness. */
export interface Item {
  id: string;
  kind: "user_message" | "agent_message" | "note" | "read" | "change" | "command" | "search" | "tool" | "question" | "subagent" | "notice";
  state: "running" | "done" | "failed";
  at: number;
  /** One line of plain text that says what it is. */
  line: string;
  /** The subagent item it happened inside. */
  inside?: string;
}

/** One message to an agent, and all it did until it stopped. */
export interface Turn {
  n: number;
  state: "running" | "done" | "failed" | "stopped";
  started: number;
  ended: number | null;
  items: Item[];
}

export interface Live {
  /** The service answers, or stopped answering only a moment ago. */
  connected: boolean;
  /** "starting" also while a voice that was on waits for the service to come back. */
  voice: "off" | "starting" | "on";
  /** Why the voice is off, when it didn't stop because the user stopped it. */
  problem: string | null;
  /** The voice is on, but the browser plays no sound before the user clicks. */
  quiet: boolean;
  tasks: Task[];
  /** The agents that exist. Left out by a service, or a loading, from before it sent them. */
  agents?: Agent[] | null;
  /** The service can show an agent's chat. Left out by one from before it could. */
  chats?: boolean;
  /** The agent whose chat is open in the side panel; none while the panel lists the agents. */
  open?: number | null;
  /** The open agent's session, once the service sent it. */
  session?: { agent: number; turns: Turn[] } | null;
  /** The marked part: what the next command is about. */
  subject: Part | null;
  /** Who marked it: the user, or the assistant. Left out by a loading from before it existed. */
  by?: "user" | "assistant" | null;
  /** The last thing heard or said. */
  heard: { who: "me" | "ai"; text: string } | null;
  /** The tasks sidebar is open. */
  panel: boolean;
  /** The scratch pad on the canvas, with a number that changes when its file does; none while the canvas is closed. */
  canvas?: { pad: string; version: number } | null;
}

/** devpage's page module, as a page has it: this page's, or the one of the pad drawn in the canvas's frame. */
type PageModule = typeof page;

/** Audio goes both ways as mono 16-bit PCM at this rate. */
const RATE = 24000;
/** After the connection closed: how long it is tried again quickly, in ms. Then the waits double up to 8 s. */
const QUICK = 20_000;
/** How long between two quick tries. */
const SOON = 400;
/** How long the widgets stay shown without a connection. */
const SHOWN = 5_000;
/** How long a voice that was on waits for the service before it gives up. */
const PATIENCE = 60_000;
/** How long the service has to say that the voice is starting, once asked. */
const ANSWER = 10_000;
/** A page loaded again within this long starts the voice again when it was on: not a tab opened tomorrow. */
const RELOAD = 15_000;
/** Where a page that goes away leaves the time, when the voice was on. */
const REMEMBERED = "live-voice-on";

// What outlives one loading of this file. devpage loads the file anew whenever it is edited, beside the one already
// running: the new one takes everything here over from the old one, so the connection, the microphone and the sound
// go on untouched, and nothing has to be reloaded.

interface Kept {
  live: Live;
  watchers: Set<() => void>;
  /** How loud each voice is right now, 0 to 1. */
  loud: { me: number; ai: () => number };
  socket: WebSocket | null;
  /** The next try to connect. */
  timer: ReturnType<typeof setTimeout> | undefined;
  /** The wait before the next slow try. */
  retry: number;
  /** When the connection closed; 0 while it is open. */
  down: number;
  /** The service said it is about to restart. */
  restarting: boolean;
  /** The user turned the voice on and has not stopped it. */
  wanted: boolean;
  /** Since when the voice waits to be started again by the page itself; 0 when it doesn't. */
  resume: number;
  /** When the service was asked to start the voice, until it says that it is starting; else 0. */
  asked: number;
  audio: AudioContext | null;
  microphone: MediaStream | null;
  capture: AudioWorkletNode | null;
  voiceMeter: AnalyserNode | null;
  /** When the AI's queued speech ends, on the audio clock. */
  speechEnds: number;
  /** What keeps the screen on while the voice is wanted. Left out by a loading from before it existed. */
  screen?: WakeLockSentinel | null;
  /** The window of the pad the canvas shows, once it is drawn. Left out by a loading from before the canvas. */
  pad?: Window | null;
}

/** Changes whenever `Kept` changes so that an older loading's could not be taken over as it is. */
const SHAPE = 1;

interface Loaded {
  shape: number;
  kept: Kept;
  /** Stops this loading: its listeners and its timer. What it kept stays as it is. */
  retire: () => void;
}

const world = globalThis as { __liveSession?: Loaded };

/**
 * This page is a scratch pad drawn inside the canvas's frame. The session's code is loaded there too, since the pad
 * is drawn as a page, and does nothing: the page around the frame has the connection, the voice and the widgets.
 */
export const framed = (() => {
  try {
    return window.frameElement?.hasAttribute("data-live-canvas") === true;
  } catch {
    return false;
  }
})();
/** A newer loading of this file has taken over. */
let retired = false;
/** What stops this loading's listeners. */
const stops: (() => void)[] = [];

function fresh(): Kept {
  return {
    live: { connected: false, voice: "off", problem: null, quiet: false, tasks: [], subject: null, heard: null, panel: false },
    watchers: new Set(),
    loud: { me: 0, ai: () => 0 },
    socket: null,
    timer: undefined,
    retry: 500,
    down: 0,
    restarting: false,
    wanted: false,
    resume: 0,
    asked: 0,
    audio: null,
    microphone: null,
    capture: null,
    voiceMeter: null,
    speechEnds: 0,
  };
}

/** What this loading works with: the older loading's, when there is one, or else new. */
function take(): Kept {
  const earlier = world.__liveSession;
  if (framed) {
    try {
      earlier?.retire();
    } catch {}
    return earlier?.shape === SHAPE ? earlier.kept : fresh();
  }
  if (!earlier) {
    const kept = fresh();
    // The page was loaded again while the voice was on: it starts again.
    try {
      const left = Number(sessionStorage.getItem(REMEMBERED));
      sessionStorage.removeItem(REMEMBERED);
      if (left && Date.now() - left < RELOAD) {
        kept.wanted = true;
        kept.resume = Date.now();
        kept.live.voice = "starting";
      }
    } catch {}
    return kept;
  }
  try {
    earlier.retire();
  } catch {}
  if (earlier.shape === SHAPE) return earlier.kept;
  // Kept in another shape: let go of what it holds, and start the voice again when it was on.
  const kept = fresh();
  const old: Partial<Kept> = earlier.kept ?? {};
  try {
    old.socket?.close();
    old.microphone?.getTracks().forEach((track) => track.stop());
    void old.audio?.close();
    void old.screen?.release().catch(() => {});
  } catch {}
  if (old.watchers instanceof Set) kept.watchers = old.watchers;
  if (old.live?.panel) kept.live.panel = true;
  if (old.wanted) {
    kept.wanted = true;
    kept.resume = Date.now();
    kept.live.voice = "starting";
  }
  return kept;
}

/** True when this loading is the page's first: nothing was kept before it. */
const first = !world.__liveSession;
const kept = take();
world.__liveSession = {
  shape: SHAPE,
  kept,
  retire() {
    retired = true;
    clearTimeout(kept.timer);
    stops.splice(0).forEach((stop) => stop());
  },
};

function set(change: Partial<Live>) {
  kept.live = { ...kept.live, ...change };
  kept.watchers.forEach((watcher) => watcher());
  awake();
}

// The screen: kept on while the voice is wanted, since the user only talks and touches nothing.

/** A request for the screen is under way, so that it is asked for once. */
let asking = false;

/**
 * Holds the screen on while the voice is wanted, and lets it go when it is not. Called on every change of state, and
 * when the tab is shown again: the browser lets the screen go whenever the tab is hidden. A browser that has no such
 * thing or refuses (no secure context, low battery) leaves the session as it is.
 */
function awake() {
  if (retired) return;
  const held = kept.screen && !kept.screen.released ? kept.screen : null;
  if (!kept.wanted) {
    kept.screen = null;
    if (held) void held.release().catch(() => {});
    return;
  }
  if (held) {
    // Taken over from an older loading: its end is answered by this loading's code.
    held.onrelease = () => letGo(held);
    return;
  }
  if (asking || document.visibilityState !== "visible" || !navigator.wakeLock) return;
  asking = true;
  navigator.wakeLock.request("screen").then(
    (lock) => {
      asking = false;
      // Stopped meanwhile, or another loading got one first: this one is not needed.
      if (!kept.wanted || (kept.screen && !kept.screen.released)) return void lock.release().catch(() => {});
      kept.screen = lock;
      lock.onrelease = () => letGo(lock);
    },
    () => {
      asking = false;
    },
  );
}

/** The browser let the screen go: it is asked for again when the tab is shown, or a moment later when it is. */
function letGo(lock: WakeLockSentinel) {
  if (kept.screen === lock) kept.screen = null;
  setTimeout(awake, 1000);
}

/** The session's state, in a widget. */
export function useLive() {
  const local = useSyncExternalStore(
    (watcher) => {
      kept.watchers.add(watcher);
      return () => kept.watchers.delete(watcher);
    },
    () => kept.live,
  );
  const work = useWorld("work");
  const selection = useWorld("devpage");
  const canvas = useWorld("canvas");
  return { ...local, ...(work && { tasks: work.tasks, agents: work.agents, chats: work.chats }),
    ...(selection && { subject: selection.subject, by: selection.by }), ...(canvas && { canvas: canvas.canvas }) } as Live;
}

/** How loud each voice is right now, 0 to 1. Read every frame by the orb. */
export const loud = kept.loud;

/** Listens on the window until a newer loading takes over. */
function listen(name: string, listener: () => void) {
  window.addEventListener(name, listener, true);
  stops.push(() => window.removeEventListener(name, listener, true));
}

// The connection.

function send(message: unknown) {
  if (kept.socket?.readyState === WebSocket.OPEN) kept.socket.send(JSON.stringify(message));
}
// A widget's call of a function of the world goes out on this connection.
parts.send = send;

function connect() {
  if (retired || framed) return;
  let address: string;
  try {
    address = base("live");
  } catch {
    return; // The document doesn't enable the session.
  }
  const opened = new WebSocket(`${location.origin.replace(/^http/, "ws")}${address}live/socket`);
  opened.binaryType = "arraybuffer";
  kept.socket = opened;
  attend(opened);
}

/** Something happened in the open agent's session, now: a turn started with a message, an item of the turn that runs is new or changed, or that turn ended. */
function added(agent: number, line: { started?: number; message?: Item; item?: Item; ended?: number; state?: Turn["state"] }) {
  const session = kept.live.session;
  if (!session || session.agent !== agent || agent !== kept.live.open) return;
  let turns = session.turns;
  const last = turns.at(-1);
  if (line.started !== undefined) {
    turns = [...turns, { n: (last?.n ?? 0) + 1, state: "running", started: line.started, ended: null, items: line.message ? [line.message] : [] }];
  } else if (last) {
    const item = line.item;
    const known = item ? last.items.some((one) => one.id === item.id) : false;
    const changed: Turn = item
      ? { ...last, items: known ? last.items.map((one) => (one.id === item.id ? item : one)) : [...last.items, item] }
      : { ...last, state: line.state ?? last.state, ended: line.ended ?? last.ended };
    turns = [...turns.slice(0, -1), changed];
  }
  set({ session: { agent, turns } });
}

/** Opens an agent's chat in the side panel, or goes back to the list of agents with `null`. */
export function openAgent(id: number | null) {
  set({ open: id, session: null });
  send({ type: "agent", id });
}

/** A message the user typed in an agent's chat: it starts a turn of that agent. */
export function tell(agent: number, text: string) {
  send({ type: "message", agent, text });
}

/** Answers a connection's events with this loading's code. */
function attend(socket: WebSocket) {
  socket.onopen = () => {
    kept.down = 0;
    kept.retry = 500;
    kept.restarting = false;
    set({ connected: true });
    tellPage();
    // The chat that was open is asked for again: a service that restarted does not know of it.
    if (kept.live.open != null) send({ type: "agent", id: kept.live.open });
  };
  socket.onmessage = (message) => {
    if (typeof message.data !== "string") return play(new Int16Array(message.data));
    const event = JSON.parse(message.data);
    if (event.type === "state" && typeof event.part === "string") {
      // One part of the world: any widget reads it with `useWorld`.
      receivedPart(event.part, event.state);
    } else if (event.type === "heard") {
      set({ heard: { who: event.who, text: event.text } });
    } else if (event.type === "session") {
      if (event.agent === kept.live.open) set({ session: { agent: event.agent, turns: event.turns } });
    } else if (event.type === "session_line") {
      added(event.agent, event.line);
    } else if (event.type === "restarting") {
      // The close that follows is a restart: the page connects again at once.
      kept.restarting = true;
    } else if (event.type === "action") {
      act(event).then(
        (value) => send({ type: "result", id: event.id, value }),
        (error) => send({ type: "result", id: event.id, value: { error: String(error) } }),
      );
    }
  };
  socket.onclose = () => closed(socket);
}

/** The connection closed, or could not be opened: it is tried again, and a voice that was on waits for it. */
function closed(socket: WebSocket) {
  if (kept.socket !== socket) return;
  kept.socket = null;
  const now = Date.now();
  kept.down ||= now;
  kept.asked = 0;
  page.pick(false);
  if (kept.wanted && now - (kept.resume || now) < PATIENCE) {
    // The microphone and the sound stay open, so starting again needs no permission and no click.
    kept.resume ||= now;
    set({ voice: "starting" });
  } else {
    const gaveUp = kept.wanted;
    kept.wanted = false;
    kept.resume = 0;
    closeAudio();
    set({ voice: "off", ...(gaveUp ? { problem: "The session's service did not come back, so the voice is off." } : {}) });
  }
  if (now - kept.down >= SHOWN && kept.live.connected) set({ connected: false });
  const quick = now - kept.down < QUICK;
  const wait = kept.restarting ? 300 : quick ? SOON : kept.retry;
  kept.restarting = false;
  if (!quick) kept.retry = Math.min(kept.retry * 2, 8000);
  clearTimeout(kept.timer);
  kept.timer = setTimeout(connect, wait);
}

/**
 * This loading came long after the page did, and no loading it could take over from was there: devpage swapped it in
 * over one written before the hand-over. Only the first state is read with this in mind. It can go once no page from
 * before 17 November 2026 is open.
 */
let swapped = first && performance.now() > 10_000;

/** Keep the browser's imperative mark and microphone controls in step with the
 * same part snapshots read by widgets. Only a voice update reconciles audio. */
function receivedPart(name: string, state: any) {
  parts.put(name, state);
  if (name === "session") return stated(state);
  if (name === "devpage") {
    set({ subject: state.subject, by: state.subject ? state.by ?? "user" : null });
    drawSubject();
  } else if (name === "canvas") {
    set({ canvas: state.canvas });
  }
}

function stated(event: Pick<Live, "voice" | "problem">) {
  let { voice, problem } = event;
  if (kept.resume) {
    if (Date.now() - kept.resume >= PATIENCE) {
      // Another session never ended: this one stays off.
      kept.resume = 0;
      kept.wanted = false;
      closeAudio();
    } else {
      // Off: the service is ready for the voice to start again. Else an earlier session is still ending.
      if (voice === "off") {
        kept.resume = 0;
        void begin(true);
      }
      voice = "starting";
      problem = null;
    }
  } else if (kept.asked) {
    if (voice !== "off") {
      kept.asked = 0;
    } else if (!problem && Date.now() - kept.asked < ANSWER) {
      // Sent before the service read the request to start: the voice is still starting.
      voice = "starting";
    } else {
      kept.wanted = false;
      kept.asked = 0;
      closeAudio();
    }
  } else if (voice === "off") {
    kept.wanted = false;
    closeAudio();
  } else if (swapped && !kept.wanted && navigator.userActivation?.hasBeenActive) {
    // A loading from before the hand-over existed holds the voice and cannot be asked: the voice is taken as this loading's.
    kept.wanted = true;
  }
  swapped = false;
  set({ voice, problem });
  page.pick(voice === "on");
  pad()?.pick(voice === "on");
  drawSubject();
}

/**
 * Marks the subject under its author's name, so that it is drawn in the author's colour: the user's in the red of the user's
 * voice, the assistant's in the blue of its voice. The colours are in DevpageLiveOrb's css.
 */
function drawSubject() {
  const { subject, by } = kept.live;
  const mine = by !== "assistant";
  for (const on of [page, pad()]) {
    on?.mark("subject-user", subject && mine ? [subject] : []);
    on?.mark("subject-assistant", subject && !mine ? [subject] : []);
  }
}

// The canvas: a scratch pad drawn in a frame over the page. devpage draws the pad as a page, so the frame has devpage's
// page module of its own, and this page uses it from outside: to hear what the user picks or selects on the pad, and to mark it.

/** What every scratch pad's address in the session starts with: a pad is `pads/<name>.md`. */
const PADS = "pads/";

/** Where devpage draws the scratch pad `pad`: the page of that name among the session's pages. */
export const padAddress = (pad: string) => `${base("live")}pages/${pad}`;

/** The page module of the pad on the canvas; null while the canvas is closed or the pad is not drawn yet. */
function pad(): PageModule | null {
  try {
    return (kept.live.canvas && (kept.pad as any)?.devpagePage?.page) || null;
  } catch {
    return null;
  }
}

/** What the user looks at: the pad while the canvas shows one, or else the page. */
const front = () => pad() ?? page;

/** What stops listening to the pad that was shown. */
let padStops: (() => void)[] = [];

/** The canvas drew a pad, in this window: what the user picks or selects there is the subject. Null when it shows none. */
export function padShown(shown: Window | null) {
  padStops.splice(0).forEach((stop) => {
    try {
      stop();
    } catch {}
  });
  kept.pad = shown;
  const inner = pad();
  if (!inner) return;
  padStops.push(inner.on("pick", (part: Part | null) => kept.live.voice === "on" && send({ type: "subject", part })));
  padStops.push(inner.on("select", (part: Part | null) => kept.live.voice === "on" && part && send({ type: "subject", part })));
  inner.pick(kept.live.voice === "on");
  drawSubject();
}
stops.push(() => padStops.splice(0).forEach((stop) => stop()));

/** Closes the canvas: the user pressed its cross. */
export function closeCanvas() {
  send({ type: "canvas", open: false });
}

// What the page tells the service, and what the service asks of the page.

/** Tells the service the page this tab shows now, and whether this is the tab the user is in. Also after each reconnect. */
function tellPage() {
  send({ type: "page", page: page.current(), focused: document.hasFocus() });
}
stops.push(page.on("change", tellPage));
listen("focus", tellPage);
stops.push(page.on("pick", (part: Part | null) => kept.live.voice === "on" && send({ type: "subject", part })));
stops.push(page.on("select", (part: Part | null) => kept.live.voice === "on" && part && send({ type: "subject", part })));

/** The part of the shown page that has `quote`, with the text around it, so the right one of several is found later. */
function find(quote: string): Part | null {
  const wanted = quote.replace(/\s+/g, " ").trim();
  const on = front();
  for (const source of on.current().sources) {
    const text = on.text(source);
    const at = text ? text.indexOf(wanted) : -1;
    if (text && at >= 0) {
      return { page: source, quote: wanted, prefix: text.slice(Math.max(0, at - 32), at), suffix: text.slice(at + wanted.length, at + wanted.length + 32) };
    }
  }
  return null;
}

/** Scrolls to parts of the shown page and marks them under `name`. */
export function show(name: string, parts: Part[], on: PageModule = front()) {
  on.mark(name, parts);
  const range = parts.map((part) => on.range(part)).find(Boolean);
  const element = range?.startContainer.parentElement;
  element?.scrollIntoView({ block: "center", behavior: "smooth" });
  return Boolean(range);
}

let pointed: ReturnType<typeof setTimeout> | undefined;

/** What scrolls the page's text: the nearest scrolling box around it, or else the window. */
export function scroller(document: Document = (pad() && kept.pad?.document) || window.document): Element {
  for (let element = document.querySelector("main h1, article h1, h1")?.parentElement; element && element !== document.body; element = element.parentElement) {
    if (element.scrollHeight > element.clientHeight + 1 && /auto|scroll/.test(getComputedStyle(element).overflowY)) return element;
  }
  return document.scrollingElement ?? document.documentElement;
}

async function act(action: { name: string; address?: string; quote?: string; to?: "down" | "up" | "top" | "bottom"; open?: boolean }) {
  switch (action.name) {
    case "read":
      // While the canvas shows a pad, the pad is what the user reads.
      return { address: front().current().address, selected: front().selection()?.quote ?? null, panel: kept.live.panel ? "open" : "closed", text: (front().text() ?? "").slice(0, 40000) };
    case "panel":
      panel(action.open === true);
      return { panel: kept.live.panel ? "open" : "closed" };
    case "go":
      await page.go(action.address!);
      return { address: page.current().address };
    case "find":
      return { part: find(action.quote!) };
    case "scroll": {
      const box = scroller();
      const screen = Math.min(box.clientHeight, window.innerHeight) * 0.85;
      const top = action.to === "top" ? 0 : action.to === "bottom" ? box.scrollHeight : box.scrollTop + (action.to === "up" ? -screen : screen);
      box.scrollTo({ top, behavior: "smooth" });
      return { scrolled: action.to };
    }
    case "point": {
      const part = find(action.quote!);
      if (!part) return { error: "the page has no such text" };
      show("pointed", [part]);
      clearTimeout(pointed);
      pointed = setTimeout(() => [page, pad()].forEach((on) => on?.mark("pointed", [])), 8000);
      return { pointed: true };
    }
  }
  return { error: `the page cannot ${action.name}` };
}

/** Goes to what a task changed, or to the part it was about, and marks it. */
export async function goTo(task: Task) {
  // A scratch pad's address is as the session says it, `pads/<name>.md`; a page of the project starts with a slash.
  const parts = (task.changes.length > 0 ? task.changes : task.part ? [task.part] : []).map((part) => ({
    ...part,
    page: part.page.startsWith("/") || part.page.startsWith(PADS) ? part.page : `/${part.page}`,
  }));
  const address = parts[0]?.page ?? task.page;
  if (!address) return;
  // What it changed is a scratch pad: the canvas shows it.
  if (address.startsWith(PADS)) {
    if (kept.live.canvas?.pad !== address) return send({ type: "canvas", open: true, address });
    return void show("changed", parts.filter((part) => part.page === address));
  }
  // A page of the project: it is under the canvas while that is open.
  if (kept.live.canvas) closeCanvas();
  if (!page.current().sources.includes(address)) await page.go(address);
  show("changed", parts.filter((part) => part.page === address), page);
}

// Audio: the user's microphone up, the AI's voice down. Open while the session is on, and while it waits to start again.

/** Runs on the audio thread: hands the microphone on in pieces of 20 ms. */
const CAPTURE = `
registerProcessor("capture", class extends AudioWorkletProcessor {
  constructor() { super(); this.piece = new Int16Array(${RATE / 50}); this.filled = 0; }
  process(inputs) {
    const samples = inputs[0][0];
    if (!samples) return true;
    for (const sample of samples) {
      this.piece[this.filled++] = Math.max(-1, Math.min(1, sample)) * 32767;
      if (this.filled === this.piece.length) {
        this.port.postMessage(this.piece.slice());
        this.filled = 0;
      }
    }
    return true;
  }
});`;

/** How long the last opening took, for the session's log: the microphone alone, then all of it. `kept` when they were still open. */
let opened = { microphone_ms: 0, audio_ms: 0, kept: true };

/** The opening under way, so that two starts at once open one microphone. */
let opening: Promise<void> | null = null;

/** Opens the microphone and the sound, or keeps them when they are still open. */
function openAudio() {
  opening ??= open().finally(() => (opening = null));
  return opening;
}

async function open() {
  const hearing = (kept.microphone?.getAudioTracks() ?? []).some((track) => track.readyState === "live");
  if (kept.audio && kept.audio.state !== "closed" && kept.capture && kept.voiceMeter && hearing) {
    opened = { microphone_ms: 0, audio_ms: 0, kept: true };
    attendAudio();
    return;
  }
  closeAudio();
  const from = performance.now();
  const microphone = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1, echoCancellation: true, noiseSuppression: false, autoGainControl: false } });
  const microphone_ms = Math.round(performance.now() - from);
  // Made after the microphone opened: a page that is capturing may be allowed sound where another is not.
  const audio = new AudioContext({ sampleRate: RATE, latencyHint: "interactive" });
  kept.microphone = microphone;
  kept.audio = audio;
  await audio.audioWorklet.addModule(URL.createObjectURL(new Blob([CAPTURE], { type: "text/javascript" })));
  const capture = new AudioWorkletNode(audio, "capture");
  audio.createMediaStreamSource(microphone).connect(capture);
  const meter = audio.createAnalyser();
  meter.fftSize = 256;
  meter.smoothingTimeConstant = 0;
  meter.connect(audio.destination);
  kept.capture = capture;
  kept.voiceMeter = meter;
  kept.speechEnds = 0;
  opened = { microphone_ms, audio_ms: Math.round(performance.now() - from), kept: false };
  attendAudio();
}

/** Answers the open audio's events with this loading's code. */
function attendAudio() {
  const { audio, capture, voiceMeter: meter } = kept;
  if (!audio || !capture || !meter) return;
  capture.port.onmessage = ({ data }: MessageEvent<Int16Array>) => {
    let sum = 0;
    for (const sample of data) sum += (sample / 32768) ** 2;
    kept.loud.me = Math.min(1, Math.sqrt(sum / data.length) * 6);
    if (kept.live.voice === "on" && kept.socket?.readyState === WebSocket.OPEN) kept.socket.send(data.buffer as ArrayBuffer);
  };
  const samples = new Float32Array(meter.fftSize);
  kept.loud.ai = () => {
    meter.getFloatTimeDomainData(samples);
    let sum = 0;
    for (const sample of samples) sum += sample * sample;
    return Math.min(1, Math.sqrt(sum / samples.length) * 6);
  };
  // A browser that wants a click before a page makes sound leaves it suspended: neither voice gets through until then.
  audio.onstatechange = () => kept.audio === audio && set({ quiet: audio.state === "suspended" });
  wake();
  // It starts a moment after it is made, so it is asked only then.
  setTimeout(() => kept.audio === audio && set({ quiet: audio.state === "suspended" }), 1000);
}

/** Starts the sound when the browser held it back: called on every click and key. */
function wake() {
  if (kept.audio?.state === "suspended") void kept.audio.resume().catch(() => {});
}
listen("pointerdown", wake);
listen("keydown", wake);
listen("visibilitychange", awake);

function closeAudio() {
  kept.microphone?.getTracks().forEach((track) => track.stop());
  kept.microphone = null;
  if (kept.audio) kept.audio.onstatechange = null;
  if (kept.audio && kept.audio.state !== "closed") void kept.audio.close().catch(() => {});
  kept.audio = null;
  kept.capture = null;
  kept.voiceMeter = null;
  kept.loud.me = 0;
  kept.loud.ai = () => 0;
  if (kept.live.quiet) set({ quiet: false });
}

/** Queues a piece of the AI's speech right after the last one. */
function play(pcm: Int16Array) {
  const { audio, voiceMeter } = kept;
  // Queued on a clock that stands still, it would all come at once later.
  if (!audio || !voiceMeter || audio.state !== "running") return;
  const buffer = audio.createBuffer(1, pcm.length, RATE);
  const samples = buffer.getChannelData(0);
  for (let at = 0; at < pcm.length; at++) samples[at] = pcm[at] / 32768;
  const source = audio.createBufferSource();
  source.buffer = buffer;
  source.connect(voiceMeter);
  kept.speechEnds = Math.max(kept.speechEnds, audio.currentTime + 0.03);
  source.start(kept.speechEnds);
  kept.speechEnds += buffer.duration;
}

/** Opens or closes the tasks sidebar. It stays closed until the user opens it. */
export function panel(open: boolean) {
  page.panel(open);
  set({ panel: open });
}
// A newer loading leaves it as the user had it.
if (first) panel(false);

/** The voice did not start. */
function failed(problem: string) {
  kept.wanted = false;
  kept.asked = 0;
  closeAudio();
  set({ voice: "off", problem });
}

/** Opens the microphone, then asks the service for the voice. `again` when the page does it by itself, after the voice was on. */
async function begin(again: boolean) {
  kept.wanted = true;
  // When the user pressed, or the page started by itself: the session's log measures the start from here.
  const pressed = (kept.asked = Date.now());
  set({ voice: "starting", problem: null });
  try {
    await openAudio();
  } catch (error) {
    // Stopped while it opened: nothing failed.
    if (!kept.wanted) return closeAudio();
    const problem = `The microphone did not open: ${error instanceof Error ? error.message : error}`;
    send({ type: "start_failed", text: problem, ms: Date.now() - pressed });
    return failed(problem);
  }
  if (!kept.wanted) return closeAudio();
  if (kept.socket?.readyState !== WebSocket.OPEN) {
    // The connection closed meanwhile: asked again when it is back.
    kept.asked = 0;
    kept.resume ||= Date.now();
    return;
  }
  const asked = (kept.asked = Date.now());
  send({ type: "start", resumed: again, timing: { pressed, ...opened } });
  setTimeout(() => {
    if (kept.asked !== asked) return;
    send({ type: "start_failed", text: "The service did not answer the request to start.", ms: Date.now() - pressed });
    failed("The voice did not start: the service did not answer.");
  }, ANSWER);
}

/** Starts the session: the microphone opens, then the voice. */
export async function start() {
  if (kept.live.voice !== "off") return;
  await begin(false);
}

/** Stops the voice. Running tasks finish. */
export function stop() {
  kept.wanted = false;
  kept.resume = 0;
  kept.asked = 0;
  send({ type: "stop" });
  closeAudio();
  awake();
  if (kept.socket?.readyState !== WebSocket.OPEN) set({ voice: "off" });
}

// A page that goes away while the voice is on leaves a note for the page loaded in its place.
listen("pagehide", () => {
  try {
    if (kept.wanted) sessionStorage.setItem(REMEMBERED, String(Date.now()));
  } catch {}
});

// A loading from before the marks had authors marked the subject under this name: this one marks it again, under its author's.
if (!first) {
  page.mark("subject", []);
  drawSubject();
}

// Taken over from an older loading: its connection and its audio now run this code. Else a first connection.
if (kept.socket && kept.socket.readyState !== WebSocket.CLOSED) attend(kept.socket);
else connect();
attendAudio();
awake();
