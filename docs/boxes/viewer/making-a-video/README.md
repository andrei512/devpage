# Making a video

How a deck becomes a narrated video: `devpage video` has GPT-Live read each
slide's notes aloud while a browser shows the deck and records it, and writes
an mp4. It is a task and mechanism article under the [viewer](../README.md).

## `devpage video`

<Terminal>

```
$ export DEVPAGE_OPENAI_API_KEY=sk-...
$ devpage video talks/launch
slide 1/6
slide 2/6
...
slide 6/6
Launch.mp4
```

</Terminal>

It takes a deck — the same argument as `devpage`, and `--root` the same way —
and writes `<title>.mp4` in the current folder unless `-o` names the file, with
the [PDF's naming rule](../pdf/README.md#devpage-pdf): characters a file name
can't hold become `-`. It prints one line on stderr as each slide starts, and
the file's name when it is done. A site, a doc or a design is refused
(`video records slides, not a doc`).

The video is 1920 × 1080, H.264 with AAC sound: each slide as the browser
shows it, devpage's own controls hidden, and the narrator's voice. The slide
changes a moment before the narrator starts on it.

## Before you run it

Two things are checked before anything slow starts, and either one missing
stops the run with an error naming what is missing:

- **A narrator.** GPT-Live, from OpenAI or Azure, chosen by environment
  variables (below).
- **Notes on every slide.** Each slide's `<Notes>` is its script; a slide
  without notes is named in the error
  (`every slide needs <Notes> to be narrated; these have none: 3 (demo.md)`).

### The narrator

devpage reads only its own `DEVPAGE_` variables — never `OPENAI_API_KEY` — so
a key set for other tools is never spent by accident.

| Variable | Selects |
| --- | --- |
| `DEVPAGE_OPENAI_API_KEY` | OpenAI. |
| `DEVPAGE_AZURE_OPENAI_ENDPOINT` + `DEVPAGE_AZURE_OPENAI_API_KEY` | Azure Foundry. The endpoint being set selects Azure, even beside an OpenAI key; without its key it is an error. |
| `DEVPAGE_VIDEO_MODEL` | The model, `gpt-live-1` by default; on Azure, the deployment's name. |

On OpenAI the session is `wss://api.openai.com/v1/live/sessions`, the key sent
as a bearer token. On Azure it is `wss://<resource host>/openai/v1/live/sessions`
— the host taken from the endpoint, given as `https://<resource>.openai.azure.com/`
(any path) or the bare host — the key in the `api-key` header. Only the OpenAI
path is exercised by a live test; the Azure address and header are checked by
unit tests alone.

**GPT-Live bills per minute** of session, and a session lasts as long as the
talk plus a few seconds per slide: a ten-minute talk costs about ten minutes.

## Writing the notes

A slide's [notes](../making-slides/README.md#speaker-notes) are said exactly
as written, word for word, so write them as speech: whole sentences, numbers
and names as they should sound, nothing meant only for the eye. The deck's
`context` is what the narrator is told before any slide — what the talk is,
who it is for, its tone:

```yaml
kind: slides
title: Launch
context: A five-minute launch talk for engineers who have never seen devpage. Warm, plain, no hype.
slides: [intro.md, problem.md, demo.md]
```

An agent writing the notes should look at each slide while it writes: every
slide is a [PNG](../making-slides/README.md#a-slide-as-an-image) at
`<deck file>/<n>.png` on a running devpage, so the script can talk about what
the slide shows.

## How it is recorded

```mermaid
sequenceDiagram
  participant video as devpage video
  participant chrome as Chrome for Testing
  participant live as GPT-Live
  video->>chrome: open the deck, served on a loopback port
  chrome->>chrome: capture its own tab, arm the recorder
  video->>live: start a session: the context and how to read a script
  loop each slide
    video->>live: say this slide's notes
    live-->>video: the voice, streamed
    video->>chrome: the voice, silences trimmed; resume recording
    video->>chrome: next slide, then pause
  end
  chrome-->>video: the mp4, in pieces
```

The page records itself. devpage serves the deck on a loopback port, as
[`devpage pdf`](../pdf/README.md#the-chrome-it-drives) does, opens it in its
own Chrome, and evaluates its recorder (`assets/video.js`) into the page: the
page captures its own tab at 1920 × 1080, plays the voice through one
continuous audio player, and records both into one `MediaRecorder`. Once
capture starts, Chrome adds a "sharing this tab" bar that shrinks the page, so
devpage grows the window back until the page measures 1920 × 1080 three times
in a row. Chrome's encoder sometimes fails to start, so the recorder is started,
checked and paused before any narration — up to five times — and the run stops
if it never comes up.

For each slide devpage asks GPT-Live for its notes, waits until the voice has
started and 2 seconds of it are buffered — the recorder is paused while it
waits, so the wait never shows — then resumes the recorder and plays it. The
voice streams in continuously, silence included, and there is no
end-of-speech event: a slide is done after 1.5 seconds without speech. Then
devpage presses → while still recording, waits the slide's lead, and only then
pauses, so the recording resumes on the new slide.

### Silence

The narrator pauses between sentences and waits between slides. What reaches
the video has **no silence longer than 1 second** (`MAX_SILENCE_SECONDS`):

- Inside a slide, a pause up to a second is kept as it is; a longer one is cut
  to a second.
- At a slide change the gap is split: the old slide keeps 0.6 seconds of it,
  and the new slide shows for 0.4 seconds (`SLIDE_LEAD_SECONDS`) before its
  voice starts. Silence before the next slide's voice is dropped.
- Every cut fades out and back in over 5 ms, so it can't click.

Speech is a 10 ms window whose loudest sample is over 500 (of 32768). These
are fixed; there is nothing to configure.

## The Chrome it drives

`devpage video` drives **Chrome for Testing**, not the chrome-headless-shell
everything else uses: the shell can't capture a tab. devpage downloads it the
first time a video is made — about 370 MB once unpacked, into
`~/Library/Caches/devpage/chrome-for-testing` on macOS,
`~/.cache/devpage/chrome-for-testing` elsewhere, beside the shell — and uses
that copy from then on, in headless mode. Each run starts its own Chrome, with
its own temporary profile and a debugging port Chrome picks, and kills it when
the run ends, however it ends — done, failed, Ctrl-C, `SIGTERM` or `SIGHUP`.
