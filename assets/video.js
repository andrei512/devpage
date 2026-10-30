// devpage video's recorder, evaluated into the deck's own page by src/video.rs.
// The page records itself: its tab, captured, and the narrator's voice, played
// through one continuous AudioWorklet player, go into one MediaRecorder. devpage
// pushes the voice in, and pauses the recorder only between slides, once the
// player has played its last sample.
window.devpageVideo = (() => {
  const TYPE = "video/mp4;codecs=avc1,mp4a.40.2";
  // The sound is recorded at 48 kHz: Windows' AAC encoder takes only 44.1
  // or 48 kHz, and the narrator speaks at a lower rate.
  const CONTEXT_RATE = 48_000;
  // One queue of samples at the narrator's rate, played without a gap for as
  // long as it holds any, resampled to the context's rate between neighbours;
  // it says "empty" when it runs out.
  const WORKLET = `
class Player extends AudioWorkletProcessor {
  constructor(options) {
    super();
    this.step = options.processorOptions.rate / sampleRate;
    this.queue = []; this.at = 0; this.empty = true;
    this.port.onmessage = (e) => { this.queue.push(e.data); this.empty = false; };
  }
  process(_, outputs) {
    const out = outputs[0][0];
    for (let i = 0; i < out.length; i++) {
      const head = this.queue[0];
      if (!head) { out[i] = 0; continue; }
      const k = Math.floor(this.at);
      const a = head[k];
      const b = k + 1 < head.length ? head[k + 1] : (this.queue[1]?.[0] ?? a);
      out[i] = a + (b - a) * (this.at - k);
      this.at += this.step;
      while (this.queue.length && this.at >= this.queue[0].length) {
        this.at -= this.queue[0].length;
        this.queue.shift();
      }
    }
    if (!this.queue.length && !this.empty) { this.empty = true; this.at = 0; this.port.postMessage("empty"); }
    return true;
  }
}
registerProcessor("player", Player);`;

  let screen, context, player, stream, recorder, chunks, done, recording;
  let empty = true;
  let emptied = () => {};
  const events = [];

  return {
    // Hides devpage's controls, starts the repaint pixel and the capture, and
    // builds the player. Needs a user gesture, for the capture.
    async start(rate) {
      const style = document.createElement("style");
      style.textContent = "#devpage-corner, #devpage-comments-toggle { display: none !important }";
      document.head.append(style);
      // Tab capture only sends a frame when the page repaints. Keep one
      // invisible pixel changing every frame, so a slide changed while the
      // recorder is paused is in the first frame after it.
      const pixel = document.createElement("div");
      pixel.style.cssText =
        "position:fixed;left:0;top:0;width:1px;height:1px;pointer-events:none;z-index:2147483647";
      document.body.append(pixel);
      let tick = 0;
      const repaint = () => {
        pixel.style.opacity = tick++ % 2 ? "0.01" : "0.02";
        requestAnimationFrame(repaint);
      };
      repaint();
      screen = await navigator.mediaDevices.getDisplayMedia({
        video: { frameRate: 30, width: 1920, height: 1080 },
        audio: false,
        preferCurrentTab: true,
      });
      screen.getVideoTracks()[0].onended = () => events.push("track ended");
      context = new AudioContext({ sampleRate: CONTEXT_RATE });
      const module = URL.createObjectURL(new Blob([WORKLET], { type: "text/javascript" }));
      await context.audioWorklet.addModule(module);
      player = new AudioWorkletNode(context, "player", { processorOptions: { rate } });
      const voice = context.createMediaStreamDestination();
      player.connect(voice);
      player.port.onmessage = (e) => {
        if (e.data !== "empty") return;
        empty = true;
        emptied();
      };
      stream = new MediaStream([...screen.getVideoTracks(), ...voice.stream.getAudioTracks()]);
    },

    // The page's size and the captured track's.
    size() {
      const track = screen?.getVideoTracks()[0].getSettings() ?? {};
      return [innerWidth, innerHeight, track.width ?? 0, track.height ?? 0];
    },

    // Chrome's encoder sometimes fails to start: arm the recorder — start it,
    // check it came up, pause it — before any narration plays, retrying until
    // it does. Returns the attempts it took.
    async arm() {
      for (let attempt = 1; attempt <= 5; attempt++) {
        recorder = new MediaRecorder(stream, {
          mimeType: TYPE,
          videoBitsPerSecond: 6_000_000,
          audioBitsPerSecond: 160_000,
        });
        chunks = [];
        recorder.ondataavailable = (e) => chunks.push(e.data);
        let failed = false;
        recorder.onerror = (e) => {
          failed = true;
          events.push("error " + e.error?.name + " " + e.error?.message);
        };
        const these = chunks;
        done = new Promise((r) => (recorder.onstop = () => r(new Blob(these, { type: TYPE }))));
        recorder.start(1000);
        await new Promise((r) => setTimeout(r, 700));
        if (!failed && recorder.state === "recording") {
          recorder.pause();
          return attempt;
        }
        if (recorder.state !== "inactive") recorder.stop();
      }
      throw new Error("the recorder never started: " + events.join("; "));
    },

    // Queues PCM16 samples, base64, for the player.
    push(base64) {
      const bytes = Uint8Array.from(atob(base64), (c) => c.charCodeAt(0));
      const pcm = new Int16Array(bytes.buffer, 0, bytes.length >> 1);
      const data = new Float32Array(pcm.length);
      for (let i = 0; i < pcm.length; i++) data[i] = pcm[i] / 32768;
      empty = false;
      player.port.postMessage(data, [data.buffer]);
    },

    play() {
      recorder.resume();
    },

    pause() {
      recorder.pause();
    },

    // Resolves once the player has played everything it was given; after 8
    // seconds, with what it knows instead.
    drain() {
      return new Promise((r) => {
        emptied = () => {
          emptied = () => {};
          r(null);
        };
        if (empty) emptied();
        setTimeout(
          () => r({ stuck: true, audio: context.state, recorder: recorder.state, events }),
          8000,
        );
      });
    },

    // Stops the recording; returns its size in bytes, read with chunk().
    async stop() {
      recorder.stop();
      screen.getTracks().forEach((t) => t.stop());
      recording = new Uint8Array(await (await done).arrayBuffer());
      return recording.length;
    },

    // `length` bytes of the recording from `at`, base64.
    chunk(at, length) {
      const bytes = recording.subarray(at, at + length);
      let s = "";
      for (let i = 0; i < bytes.length; i += 0x8000) {
        s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
      }
      return btoa(s);
    },

    // The slide shown, as `#n` counts it.
    shown() {
      return [...document.querySelectorAll(".slide.shown")].map((s) => s.dataset.slide).join(",");
    },
  };
})();
