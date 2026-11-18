---
page-type: mechanism
---
# Voice to Fast

In client mode (`delegation: { type: "client" }`) the voice model sends the
session's program transcript fragments and a hand-over event. The program
builds Fast's input from them. The code is in `live/voice.ts`; the page
belongs to [the agents](../README.md).

The sources are OpenAI's Live API reference and its delegation guide. Quotes
are theirs.

## What arrives

### Transcript fragments

Two events, with the same fields:

| Event | Whose speech |
|---|---|
| `session.input_transcript.delta` | Yours |
| `session.output_transcript.delta` | Voice's |

| Field | What it is |
|---|---|
| `type` | Which of the two events it is. Nothing else tells the speaker. |
| `event_id` | Unique id of this fragment event. |
| `delta` | "The transcript text fragment for the audio in this time range." |
| `start_ms`, `end_ms` | Start and end on the session timeline, "in milliseconds from the beginning of the session". |
| `client_event_id` | Optional. The `event_id` of the command the program sent, when the event answers one. |

The reference's example:

```json
{
  "type": "session.input_transcript.delta",
  "event_id": "evt_input_transcript_001",
  "delta": "A table for two at seven, please.",
  "start_ms": 1600,
  "end_ms": 3400
}
```

### The hand-over

`session.delegation.created`, sent when Voice hands work over.

| Field | What it is |
|---|---|
| `delegation.id` | Id of the hand-over. The program sends it back as `delegation_id` when it replies. |
| `delegation.target` | `"client"` |
| `delegation.type` | `"delegation"` |
| `offset_ms` | Position on the same session timeline where the hand-over was created, in milliseconds from the beginning of the session. |
| `event_id` | Unique id of this event. |

The `delegation` object "contains metadata, not the task text".

The reference's example:

```json
{
  "type": "session.delegation.created",
  "event_id": "evt_delegation_001",
  "offset_ms": 3600,
  "delegation": {
    "id": "del_abc123",
    "type": "delegation",
    "target": "client"
  }
}
```

## What does not arrive

- No message id, utterance id or turn id. Only one `event_id` per fragment.
- No end of turn. The events "do not define complete turns or include a
  transcript-done event".
- No revisions are described. Fragments are appended "in delivery order".
- No guarantee the text is right: "transcripts may contain mistakes".
- No link from a hand-over to the fragments it is about. Only the shared
  timeline.

## What the program builds

| Piece | How |
|---|---|
| Messages with stable ids | Fragments are grouped into the program's own messages. Each keeps: who, the text, `start_ms` of the first fragment, `end_ms` of the last. |
| Which message a hand-over is about | By arrival: the fragments that are not a message yet when the hand-over comes. |

Limits:

- A cut-off does not prove the words are complete or final.
- A hand-over does not necessarily map to a clean interval of fragments.

How it is built:

- **Voice hands over every time you stop speaking,** for everything you say.
  It is told never to judge whether something is worth handing over, and not
  to hand over a cough or another noise without words.
- **A hand-over's words** are your fragments that are not a message yet when
  `session.delegation.created` arrives, plus those that arrive until your
  transcript has been quiet for 80 ms, at most 1.5 seconds. `offset_ms` is
  logged and not used for the cut.
- **The automatic "one moment" is cancelled.** As the hand-over arrives, the
  program sends an empty `session.thinking.append` with its id. The session
  also asks the API to wait 800 ms before it acknowledges by itself, the most
  the API accepts.
- **A second hand-over before the first is cut:** the words so far go with
  the first.
- **A hand-over whose words already became a message** is paired with that
  message.
- **A hand-over that came with no words** takes the next thing you say.
- **Without a hand-over,** your fragments become a message after a pause of
  3.5 seconds, and Voice's after 2 seconds.
- **Every fragment is logged** with its `event_id`, `start_ms` and `end_ms`.

## What Fast can do after

- The program merges the fragments of one hand-over into one message, with an
  id.
- Fast only writes code, through one tool, `exec`.
- In the script the message is a variable. Fast does not retype your words.

What Fast sees in its context:

```
m40  voice  "I've opened the waiting list page."
m41  user  "Why is the third step marked?"  page /docs/waiting-list/README.md
```

Fast's script:

```js
spawn(`The user asked: "${m41}"`);
say("I'm checking what marks that step.");
```

What a script can use is on [the world page](../world/README.md#what-a-script-can-use).

How the reply returns:

| The program sends | With | Effect |
|---|---|---|
| `session.commentary.append` | `delegation_id`, `content` | Voice speaks it. It "is trained to paraphrase the text". |
| `session.thinking.append` | `delegation_id`, `content` | Context for Voice. Not spoken. The program sends only the empty one that cancels the acknowledgement. |

Voice is told to say a line completely and unchanged. Verbatim speech is
still not guaranteed: the acknowledgement `session.commentary.appended`
comes "without guaranteeing exact wording or completed audio playback".

A line is at most 1,600 characters: the API takes 500 tokens for one, and a
longer line is cut there.
