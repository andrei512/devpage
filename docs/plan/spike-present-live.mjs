// Spike: GPT-Live narrates a deck open in headless Chrome, and Chrome records its tab to mp4.
// The voice plays in the page through one continuous AudioWorklet player, so the tab recording
// carries it. Silence is capped at MAX_SILENCE_SECONDS. The recorder pauses only between
// slides, after the player has played its last sample, so waiting for GPT-Live never shows.
// node present-live.mjs <deck url> <out.mp4>
import { spawn } from "node:child_process";
import fs from "node:fs";

const MAX_SILENCE_SECONDS = 1;
// Of that, how long the next slide shows before its voice starts; the rest stays on the old slide.
const SLIDE_LEAD_SECONDS = 0.4;
const RATE = 24000;
const WINDOW = RATE / 100; // 10 ms
const LOUD = 500; // a window whose peak is above this is speech
const DONE_AFTER_SECONDS = 1.5; // the voice is done with a slide after this much quiet
const PREBUFFER_SECONDS = 2; // audio held before a slide starts playing; covers network stalls
const PUSH_EVERY_MS = 100;
const FADE = RATE / 200; // 5 ms fade on each side of a cut

const CONTEXT =
  "You are the narrator of a recorded product presentation about devpage, a tool that turns a " +
  "folder of Markdown into a live docs site. The audience is developers. There is no one on the " +
  "line; ignore any input audio. When you are given a slide's script, say it exactly as written, " +
  "word for word, in a warm, confident presenter's voice with natural pacing. Add nothing before " +
  "or after it and ask no questions. After a script, stay silent until you get the next one.";

const SCRIPTS = [
  "Every team writes docs in Markdown, and every team ends up with docs nobody trusts. They drift " +
    "from the code, the diagrams are screenshots from last year, and review happens in a chat " +
    "thread that's gone by Friday.",
  "devpage fixes that with one command. Point it at a folder of Markdown and you get a live site: " +
    "diagrams drawn from text, links checked as you type, and pages that update the moment you save.",
  "And comments live right next to the docs, as plain files. So your agent can read every open " +
    "thread, fix the page, and resolve it. That's devpage: docs you can actually trust.",
];

const [url, out] = process.argv.slice(2);
const T0 = Date.now();
const log = (kind, fields = {}) => console.log(JSON.stringify({ t: (Date.now() - T0) / 1000, kind, ...fields }));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// --- Chrome --------------------------------------------------------------------------------
const profile = "/tmp/live-" + process.pid;
const chrome = spawn(process.env.CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", [
  "--headless=new", "--remote-debugging-port=0", "--window-size=1920,1167",
  "--user-data-dir=" + profile, "--hide-scrollbars",
  "--auto-accept-this-tab-capture", "--autoplay-policy=no-user-gesture-required", "about:blank",
]);
// This run's Chrome dies with it, whatever ends the run.
const killChrome = () => { try { chrome.kill("SIGKILL"); } catch {} };
process.on("exit", killChrome);
for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) process.on(signal, () => process.exit(1));
setTimeout(() => { log("timeout"); process.exit(1); }, 180_000).unref();
// Port 0: Chrome picks a free port and writes it to DevToolsActivePort, so no run can
// connect to another run's Chrome.
let port;
while (!port) { await sleep(100); try { port = fs.readFileSync(profile + "/DevToolsActivePort", "utf8").split("\n")[0]; } catch {} }
const target = (await (await fetch(`http://127.0.0.1:${port}/json`)).json()).find((t) => t.type === "page");
const cdp = new WebSocket(target.webSocketDebuggerUrl);
let cdpId = 0; const pending = new Map();
cdp.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); } };
await new Promise((r) => (cdp.onopen = r));
const send = (method, params = {}) => new Promise((r) => { const i = ++cdpId; pending.set(i, r); cdp.send(JSON.stringify({ id: i, method, params })); });
const evaluate = async (expression) => {
  const r = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true, userGesture: true });
  if (r.result?.exceptionDetails) throw new Error(JSON.stringify(r.result.exceptionDetails));
  return r.result?.result?.value;
};

// The capture is the page area, so make that exactly 1920x1080 before capturing.
await send("Page.enable");
await send("Page.navigate", { url });
await sleep(2500);
const { windowId } = (await send("Browser.getWindowForTarget", { targetId: target.id })).result;
log("page", { size: await evaluate("[innerWidth, innerHeight]") });

const WORKLET = `
class Player extends AudioWorkletProcessor {
  constructor() { super(); this.queue = []; this.offset = 0; this.empty = true;
    this.frames = 0;
    this.port.onmessage = (e) => { this.queue.push(e.data); this.empty = false; }; }
  process(_, outputs) {
    const out = outputs[0][0];
    for (let i = 0; i < out.length; i++) {
      const head = this.queue[0];
      if (!head) { out[i] = 0; continue; }
      out[i] = head[this.offset++];
      if (this.offset >= head.length) { this.queue.shift(); this.offset = 0; }
    }
    if (!this.queue.length && !this.empty) { this.empty = true; this.port.postMessage("empty"); }
    if ((this.frames += out.length) % 12032 < out.length)
      this.port.postMessage({ queued: this.queue.reduce((n, q) => n + q.length, 0) - this.offset, frames: this.frames, empty: this.empty });
    return true;
  }
}
registerProcessor("player", Player);`;

await evaluate(`(async () => {
  const style = document.createElement("style");
  style.textContent = "#devpage-corner, #devpage-comments-toggle { display: none !important }";
  document.head.append(style);
  // Tab capture only sends a frame when the page repaints. Keep one invisible pixel changing
  // every frame, so a slide changed while the recorder is paused is in the first frame after it.
  const pixel = document.createElement("div");
  pixel.style.cssText = "position:fixed;left:0;top:0;width:1px;height:1px;pointer-events:none;z-index:2147483647";
  document.body.append(pixel);
  let tick = 0;
  const repaint = () => { pixel.style.opacity = (tick++ % 2) ? "0.01" : "0.02"; requestAnimationFrame(repaint); };
  repaint();
  const screen = await navigator.mediaDevices.getDisplayMedia({
    video: { frameRate: 30, width: 1920, height: 1080 }, audio: false, preferCurrentTab: true });
  const ctx = new AudioContext({ sampleRate: ${RATE} });
  await ctx.audioWorklet.addModule(URL.createObjectURL(new Blob([${JSON.stringify(WORKLET)}], { type: "text/javascript" })));
  const player = new AudioWorkletNode(ctx, "player");
  const dest = ctx.createMediaStreamDestination();
  player.connect(dest);
  let emptied = () => {};
  let empty = true;
  player.port.onmessage = (e) => {
    if (e.data !== "empty") { window.__stat = e.data; return; }
    empty = true; window.__empties = (window.__empties || 0) + 1; emptied(); };
  const stream = new MediaStream([...screen.getVideoTracks(), ...dest.stream.getAudioTracks()]);
  const type = "video/mp4;codecs=avc1,mp4a.40.2";
  window.__rec = [];
  screen.getVideoTracks()[0].onended = () => window.__rec.push("track ended");
  let recorder, chunks, done;
  // Chrome's encoder sometimes fails to start; arm the recorder (start, check, pause) before any
  // narration plays, and retry until it comes up.
  window.__arm = async () => {
    for (let attempt = 1; attempt <= 5; attempt++) {
      recorder = new MediaRecorder(stream, { mimeType: type, videoBitsPerSecond: 6_000_000, audioBitsPerSecond: 160_000 });
      chunks = []; recorder.ondataavailable = (e) => chunks.push(e.data);
      let failed = false;
      recorder.onerror = (e) => { failed = true; window.__rec.push("error " + e.error?.name + " " + e.error?.message); };
      done = new Promise((r) => (recorder.onstop = () => r(new Blob(chunks, { type }))));
      recorder.start(1000);
      await new Promise((r) => setTimeout(r, 700));
      if (!failed && recorder.state === "recording") { recorder.pause(); return attempt; }
      if (recorder.state !== "inactive") recorder.stop();
    }
    throw new Error("the recorder never started: " + window.__rec.join("; "));
  };
  window.__push = (b64) => {
    const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    const pcm = new Int16Array(bytes.buffer, 0, bytes.length >> 1);
    const data = new Float32Array(pcm.length);
    for (let i = 0; i < pcm.length; i++) data[i] = pcm[i] / 32768;
    empty = false;
    player.port.postMessage(data, [data.buffer]);
  };
  window.__play = () => recorder.resume();
  // Pause once the player has played everything it was given.
  window.__pause = () => recorder.pause();
  window.__drain = () => new Promise((r) => {
    emptied = () => { emptied = () => {}; r(); };
    if (empty) emptied();
    setTimeout(() => r({ stuck: true, empty, stat: window.__stat, ctx: ctx.state, time: ctx.currentTime, recorder: recorder.state, events: window.__rec }), 8000);
  });
  window.__stop = async () => { recorder.stop(); screen.getTracks().forEach((t) => t.stop());
    const bytes = new Uint8Array(await (await done).arrayBuffer()); let s = "";
    for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    return btoa(s); };
  window.__screen = screen;
})()`);
// Capturing adds a "sharing this tab" bar that shrinks the page; grow the window back to 1920x1080.
// The bar arrives a moment after capture starts, so keep correcting until the page holds still.
for (let settled = 0, tries = 0; settled < 3 && tries < 40; tries++) {
  await sleep(250);
  const inner = await evaluate("[innerWidth, innerHeight]");
  if (inner[0] === 1920 && inner[1] === 1080) { settled++; continue; }
  settled = 0;
  const { bounds } = (await send("Browser.getWindowBounds", { windowId })).result;
  await send("Browser.setWindowBounds", { windowId, bounds: { width: bounds.width + 1920 - inner[0], height: bounds.height + 1080 - inner[1] } });
}
log("recorder", { attempts: await evaluate("window.__arm()") });
log("capture", { page: await evaluate("[innerWidth, innerHeight]"),
  track: await evaluate("(() => { const s = window.__screen.getVideoTracks()[0].getSettings(); return [s.width, s.height]; })()") });

// --- Silence cap -----------------------------------------------------------------------------
// Windows of the raw stream go in; what comes out has no silence longer than the cap, and every
// cut fades out and back in so it can't click.
let silentRun = MAX_SILENCE_SECONDS * RATE; // samples of silence in a row, across slides; the start counts as silent
let cutting = false;
let lastLoud = 0; // Date.now() of the last loud window
let lastPeak = 0;
let kept = 0, dropped = 0;
let outbox = []; // Int16Array windows waiting to be pushed

// Silent windows wait in `held` until we know whether speech follows (they're kept, up to
// the cap) or the slide ends (endSlide keeps only the old slide's share of the gap).
let held = [];

function take(pcm) {
  for (let i = 0; i < pcm.length; i += WINDOW) {
    const window = Int16Array.from(pcm.subarray(i, i + WINDOW));
    let peak = 0; for (const s of window) peak = Math.max(peak, Math.abs(s));
    if (peak > LOUD) {
      silentRun = 0; lastLoud = Date.now(); lastPeak = peak;
      if (cutting) { fade(window, true); cutting = false; }
      for (const w of [...held, window]) { outbox.push(w); kept += w.length; }
      held = [];
      continue;
    }
    silentRun += window.length;
    if (silentRun <= MAX_SILENCE_SECONDS * RATE) { held.push(window); continue; }
    if (!cutting) { const last = held.at(-1) || outbox.at(-1); if (last) fade(last, false); }
    cutting = true; dropped += window.length;
  }
}

// The slide's voice is done: keep the old slide's share of the gap, drop the rest, and drop
// whatever silence comes before the next slide's voice.
function endSlide() {
  let room = (MAX_SILENCE_SECONDS - SLIDE_LEAD_SECONDS) * RATE;
  for (const w of held) {
    if (room >= w.length) { outbox.push(w); kept += w.length; room -= w.length; } else dropped += w.length;
  }
  if (outbox.length) fade(outbox.at(-1), false);
  held = [];
  silentRun = MAX_SILENCE_SECONDS * RATE;
  cutting = true;
}

function fade(window, fadeIn) {
  const n = Math.min(FADE, window.length);
  for (let i = 0; i < n; i++) {
    const at = fadeIn ? i : window.length - n + i;
    window[at] = Math.round(window[at] * (fadeIn ? i / n : 1 - i / n));
  }
}

const buffered = () => outbox.reduce((n, w) => n + w.length, 0) / RATE;
let pushing = false;
async function flush(play = false) {
  if (!outbox.length) return;
  const joined = new Int16Array(outbox.reduce((n, w) => n + w.length, 0));
  let at = 0; for (const w of outbox) { joined.set(w, at); at += w.length; }
  outbox = [];
  // play: resume the recorder in the same tick the audio lands, so neither leads the other.
  await evaluate(`${play ? "window.__play();" : ""}window.__push(${JSON.stringify(Buffer.from(joined.buffer).toString("base64"))})`);
}

// --- GPT-Live ------------------------------------------------------------------------------
const live = new WebSocket("wss://api.openai.com/v1/live/sessions", {
  headers: { Authorization: "Bearer " + process.env.OPENAI_API_KEY },
});
await new Promise((r, j) => { live.onopen = r; live.onerror = j; });
const say = (event) => live.send(JSON.stringify(event));
let started; const ready = new Promise((r) => (started = r));
const raw = []; // everything GPT-Live sent, for comparing against the recording

live.onmessage = (m) => {
  const ev = JSON.parse(m.data);
  if (ev.type === "session.started") { log("started"); started(); }
  else if (ev.type === "session.output_audio.delta") {
    const bytes = Buffer.from(ev.delta, "base64");
    raw.push(bytes);
    take(new Int16Array(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.length)));
  } else if (ev.type === "session.output_transcript.delta") process.stdout.write(ev.delta);
  else if (ev.type === "error") log("error", { error: ev.error });
};

say({ type: "session.start", event_id: "start", session: {
  model: "gpt-live-1", instructions: CONTEXT,
  audio: { format: { type: "audio/pcm", rate: RATE }, output: { voice: "marin" } },
  delegation: { type: "client" },
} });
await ready;
const silence = Buffer.alloc(RATE / 50 * 2).toString("base64");
const feeder = setInterval(() => say({ type: "session.input_audio.append", audio: silence }), 20);

for (const [i, script] of SCRIPTS.entries()) {
  const begin = Date.now();
  outbox = []; // whatever silence arrived while waiting is not part of this slide
  say({ type: "session.instructions.append", event_id: `slide_${i + 1}`, delegation_id: null,
    content: "Now say this slide's script exactly and in full:\n\n" + script });
  // Hold the start until the voice is speaking and half a second is buffered.
  while (lastLoud < begin || buffered() < PREBUFFER_SECONDS) await sleep(10);
  await flush(true);
  log("slide", { n: i + 1 });
  let waited = 0;
  while (Date.now() - lastLoud < DONE_AFTER_SECONDS * 1000) {
    await sleep(PUSH_EVERY_MS); await flush();
    if ((waited += PUSH_EVERY_MS) % 5000 === 0) log("speaking", { sinceLoudMs: Date.now() - lastLoud, peak: lastPeak });
  }
  endSlide();
  await flush();
  const drained = await evaluate("window.__drain()");
  if (drained) log("drain", drained);
  console.log(); log("next_slide", { n: i + 1, playerEmptied: await evaluate("window.__empties") });
  if (i + 1 < SCRIPTS.length) {
    // Change the slide while recording, and pause only after the new slide has been on screen
    // for its lead: the recording resumes on whatever it last saw, so that must be the new slide.
    await send("Input.dispatchKeyEvent", { type: "keyDown", key: "ArrowRight", code: "ArrowRight", windowsVirtualKeyCode: 39 });
    await send("Input.dispatchKeyEvent", { type: "keyUp", key: "ArrowRight", code: "ArrowRight", windowsVirtualKeyCode: 39 });
    await sleep(SLIDE_LEAD_SECONDS * 1000);
    await evaluate("window.__pause()");
  }
}

clearInterval(feeder);
say({ type: "session.close", event_id: "close" });
const b64 = await evaluate("window.__stop()");
fs.writeFileSync(out, Buffer.from(b64, "base64"));
fs.writeFileSync(out.replace(/\.mp4$/, ".raw.pcm"), Buffer.concat(raw));
log("wrote", { out, bytes: fs.statSync(out).size, audioSeconds: kept / RATE, droppedSilenceSeconds: dropped / RATE });
chrome.kill(); live.close(); process.exit(0);
