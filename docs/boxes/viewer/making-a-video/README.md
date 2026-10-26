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
- **Notes on every slide.** Each slide is narrated from its `<Notes>`; a
  slide without notes is named in the error
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
and names as they should sound, nothing meant only for the eye. Write for
the ear: short sentences, questions to the listener, a full stop where a pause
belongs. Give each slide a `delivery` for how it should feel, and the deck its
`audience`, `goal` and `tone` — [what the narrator is told](#what-the-narrator-is-told)
shows where each one goes.

### What the narrator is told

Two kinds of message, and nothing else: the narrator never sees a slide's
picture or title. Everything outside `{…}` is fixed in devpage; the deck fills
in the rest. The shape follows OpenAI's GPT-Live prompt template: who the
narrator is, the talk, how to speak, its task, then the backchannel,
interruption and delegation policies.

**Once, when the session starts:**

```text
You are the narrator of "{title}", a recorded talk of {count} slides.

Audience: {audience}
Goal: {goal}
Tone: {tone}

Read like a speaker who enjoys giving this talk, in the tone above, for this audience. Speak clearly at an unhurried pace. Lean into the words that carry each slide's point.
{pronounce, one line each}

Each slide arrives as a passage with its delivery. Read the passage word for word once, with natural phrasing and complete word endings, in that delivery. Say only the passage, without an introduction, closing remark, transition, comment, or delegation. Then stay silent until the next passage arrives.

Backchannel policy: Never backchannel.
Interruption policy: There is no live listener. Input audio is only silence or noise. Do not treat a cough, music, or any sound as speech or a new request.
Delegation policy: NEVER GENERATE <|SpawnThinking|>, CHECK, ANSWER, SEARCH, OR USE TOOLS.
```

**Then once per slide, as it comes up:**

```text
Slide {n} of {count}. Delivery: {delivery}
Immediately read this passage aloud, word for word, then stop:

{notes}
```

Where each part comes from:

| Part | From | If missing |
| --- | --- | --- |
| `title` | the deck's `title` | — |
| `audience` | the deck's `audience`: who watches | `devpage video` stops and names it |
| `goal` | the deck's `goal`: what the talk should achieve | `devpage video` stops and names it |
| `tone` | the deck's `tone`: the energy of the whole talk | "Natural and upbeat, like a speaker who enjoys this talk. Engaged and warm, never flat and never salesy." |
| `pronounce` | the deck's `pronounce`: one line per word a voice gets wrong | no lines |
| `delivery` | the slide's `<Notes delivery="…">`: how this slide should feel | "in the talk's tone." |
| `notes` | the slide's `<Notes>` | `devpage video` stops and names the slide |

```yaml
kind: slides
title: devpage
audience: Software developers who use AI coding agents. Many are not native English speakers.
goal: They understand what devpage is, why it helps them, and try it today.
tone: Natural and upbeat, like a speaker who enjoys this talk. Engaged and warm, never salesy.
pronounce:
  - Say "devpage" as "dev page".
slides: [title.md, agents-write.md, hard-to-read.md]
```

```markdown
<Notes delivery="Honest and a little teasing: you have caught them.">
But be honest. Do you read them? …
</Notes>
```

A delivery is a direction, never said: "curious", "relief and quiet
confidence", "a small smile — a surprise saved for the end". Across a deck
they give the talk its arc. Exact wording is still not guaranteed by the
model, so listen to the result, or transcribe it and compare it with the
notes. A test keeps this page and the code the same text.

An agent writing the notes should look at each slide while it writes: every
slide is a [PNG](../making-slides/README.md#a-slide-as-an-image) at
`<deck file>/<n>.png` on a running devpage, so the notes can talk about what
the slide shows.

## How it is recorded

```mermaid
sequenceDiagram
  participant video as devpage video
  participant chrome as Chrome for Testing
  participant live as GPT-Live
  video->>chrome: open the deck, served on a loopback port
  chrome->>chrome: capture its own tab, arm the recorder
  video->>live: start a session: the talk (audience, goal, tone) and how to read
  loop each slide
    video->>live: ask for this slide: its delivery and notes
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
