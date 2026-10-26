# 02 — `devpage video`

Read [the plan](README.md) first — every rule in "What the prototype proved"
is required, and `spike-present-live.mjs` is the reference implementation to
port. Builds on 01 (notes and `context` are on the document model).

## What

`devpage video <PATH> [-o OUTPUT] [--root ROOT]`, beside `devpage pdf` and
shaped like it: `PATH` is a slides document; anything else is refused with a
clear message; `-o` defaults to `<title>.mp4` (the pdf naming rule, `.mp4`).

1. **Before anything slow:** read the provider from the `DEVPAGE_` env vars
   (rules in the plan); none set → fail naming the variables. Every slide must
   have notes → otherwise fail naming the slides without them.
2. **Serve** the deck on a loopback port the way `devpage pdf` does.
3. **Browser:** Chrome for Testing, fetched once into devpage's cache folder
   next to the shell (chromiumoxide's fetcher), its own temp profile per run
   (the pdf `Profile` pattern), killed on every exit path.
4. **Page side:** the recorder/player script as a file under `assets/` (not a
   Rust string), evaluated into the page: capture, AudioWorklet player,
   MediaRecorder, repaint pixel, arm/retry, push/drain/pause/resume/stop.
   Hand the finished mp4 back in chunks (a long talk is tens of MB).
5. **GPT-Live client** over WebSocket (add `tokio-tungstenite` with rustls,
   or whatever fits the existing TLS stack), OpenAI or Azure per the plan;
   instructions = narrator rules + the deck's `context`.
6. **Silence** as its own pure module with the constants
   (`MAX_SILENCE_SECONDS`, `SLIDE_LEAD_SECONDS`, and the done/loud/prebuffer
   values) and unit tests: cap inside a slide, the split at a slide's end,
   leading silence dropped, fades at cuts.
7. **Progress** on stderr: one line per slide; the output path at the end.

## Docs (same commit as the code)

- A task article `docs/boxes/viewer/making-a-video/README.md`: what it does,
  the env vars, writing notes (and looking at `<n>.png` while writing them),
  `context:`, the silence rules, the Chrome for Testing download, cost
  (GPT-Live bills per minute).
- The CLI reference / installing pages where commands and downloads are listed.
- Configuration reference: the env vars.
- The constraint `devpage-never-writes-a-document`: `video` writes its output
  file, like `pdf`.
- Feature catalog entry. Viewer README / landing card if commands are listed
  there. Do **not** touch `docs/talks/`.

## Tests

Unit: silence module, provider config from env (each case incl. none set and
Azure without key), URL building, the missing-notes check, refusing a non-deck.
Chrome (`#[ignore]`): record a 2-slide fixture deck with a synthetic voice fed
through the same player (no API), then check the file is mp4, 1920×1080, has
an audio track, and the slide changed (decode with AVFoundation is not
available in Rust — checking the mp4 box structure for both tracks and the
`tkhd` size is enough). Live (`#[ignore]`, skips without
`DEVPAGE_OPENAI_API_KEY`): the fixture deck end to end.

## Done when

Gates green (fmt, clippy `-D warnings`, `cargo test`, the ignored Chrome tests
you added), and **one real run**: `devpage video` on a 3-slide fixture with
the operator's key (`DEVPAGE_OPENAI_API_KEY="$OPENAI_API_KEY"`; never print
it), measured with `audiocheck.py` / `gaps.py`: no dropouts, no silence over
1.05 s, 1920×1080. Committed on your lane branch.
