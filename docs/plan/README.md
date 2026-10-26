# Plan: narrated video from a deck

`devpage video` turns a slides deck into an mp4: GPT-Live reads each slide's
speaker notes word for word while a real browser shows the deck and records
its own tab. Deleted once every lane is merged.

Story:

1. A slide carries its script in `<Notes>`, a deck its `context:`, and every
   slide is also a PNG an agent can look at while it writes the script.
   ([01](01-notes-context-png.md))
2. `devpage video` records the deck, narrated, to `<title>.mp4`.
   ([02](02-video.md))

02 builds on 01; they run one after the other.

## What the prototype proved (keep it)

`spike-present-live.mjs` is the working prototype (Node, run against a deck
served by devpage). Measured on five runs in a row: 1920×1080 H.264 + AAC,
no dropouts in speech, no silence over 1.0 s, each new slide on screen 0.4 s
before its voice. Every rule below is something that broke in the prototype
when it wasn't followed.

- **GPT-Live, scripted, no second model.** `wss://api.openai.com/v1/live/sessions`,
  `session.start` with `model`, `instructions` (the deck's context plus the
  narrator rules), `audio.format` PCM16 24 kHz, `audio.output.voice` `marin`,
  `delegation: {type: "client"}`. Each slide: `session.instructions.append`
  with `delegation_id: null` and "Now say this slide's script exactly and in
  full:\n\n<notes>". GPT-Live needs a continuous input stream: send 20 ms of
  silence every 20 ms (`session.input_audio.append`). Audio arrives as
  `session.output_audio.delta`; it streams *continuously*, silence included,
  and there is no end-of-speech event — "done" is 1.5 s with no 10 ms window
  whose peak is over 500.
- **Chrome for Testing, not chrome-headless-shell.** The shell has no tab
  capture (`getDisplayMedia` → NotSupportedError). Chrome for Testing in
  `--headless=new` has it and encodes `video/mp4;codecs=avc1,mp4a.40.2`.
  Flags: `--auto-accept-this-tab-capture`,
  `--autoplay-policy=no-user-gesture-required`, a window of 1920×1167.
  Downloaded once into devpage's cache like the shell; only `video` uses it.
- **The page records itself.** `getDisplayMedia({video:{frameRate:30,width:1920,height:1080},preferCurrentTab:true})`
  (needs `userGesture: true` on the evaluate), plus the voice played through
  one continuous AudioWorklet player into a `MediaStreamAudioDestinationNode`,
  into one `MediaRecorder` (6 Mbit/s video, 160 kbit/s audio).
- **Frame size.** Capturing adds a "sharing this tab" bar a moment later that
  shrinks the page; keep correcting the window bounds until `innerWidth/innerHeight`
  read 1920×1080 three times in a row, *after* capture starts.
- **Repaint.** Tab capture only emits a frame on repaint: keep one invisible
  pixel changing every animation frame.
- **Arm the recorder.** Chrome's encoder sometimes fails to start
  ("Encoder initialization failed", recorder goes inactive): start, wait
  700 ms, check for error, pause; retry up to 5 times before any narration.
- **Silence.** `MAX_SILENCE_SECONDS = 1` and `SLIDE_LEAD_SECONDS = 0.4` are
  constants. Silent windows are held back until speech resumes (kept, up to
  the cap) or the slide ends (the old slide keeps MAX − LEAD of it); the rest
  is dropped; leading silence of the next slide is dropped; 5 ms fades at
  every cut.
- **Buffer and pause.** Hold 2 s of audio before a slide starts playing
  (the recorder is paused while waiting, so it costs nothing in the video).
  The recorder pauses only between slides.
- **The slide changes while recording.** After the old slide's audio has
  played out: press next, wait `SLIDE_LEAD_SECONDS`, *then* pause. After a
  resume the video shows what was on screen at the pause for ~0.7 s, so that
  must already be the new slide.
- **Hide devpage's controls** (`#devpage-corner`, `#devpage-comments-toggle`).
- **One browser per run**, killed when the run ends however it ends; never
  reuse a fixed debugging port. (Leftover Chromes on a fixed port were the
  cause of every "glitch" in the prototype.)

`audiocheck.py` and `gaps.py` measure a recording (convert with
`afconvert -f WAVE -d LEI16@24000 -c 1 x.mp4 x.wav`): longest silences,
dropouts inside speech, and the gaps at slide changes.

## Decided with the operator

- Notes are each slide's exact script, in a `<Notes>` tag in the slide's
  `.md`; never rendered on the slide. The agent that edits the deck writes
  them, looking at each slide's PNG.
- The deck's `context:` (what the talk is, audience, tone) lives in the
  deck's yml and becomes the narrator's instructions.
- A slide's PNG is at `<deck>.yml/<n>.png` (1-based, like `#n`).
- `devpage video <deck yml>` writes `<title>.mp4` by default; `-o` and
  `--root` as for `devpage pdf`.
- Provider from env vars only, `DEVPAGE_` prefix only, no fallback to
  `OPENAI_API_KEY`; fail if no provider is set:
  - `DEVPAGE_OPENAI_API_KEY` — OpenAI.
  - `DEVPAGE_AZURE_OPENAI_ENDPOINT` + `DEVPAGE_AZURE_OPENAI_API_KEY` — Azure
    Foundry; the endpoint being set selects Azure. URL:
    `wss://<resource host>/openai/v1/live/sessions`, key in the `api-key`
    header (Microsoft Learn: "Use GPT-Live for real-time voice").
  - `DEVPAGE_VIDEO_MODEL` — optional, default `gpt-live-1`; on Azure the
    deployment name.
- Silence trimming with the two constants above.
- Out of scope: the presenter window for humans (still open what it shows),
  the pitch deck (`docs/talks/` — the operator iterates on it themselves; do not
  touch it), silence editing beyond the constants.
