---
name: presentation
description: Make a clear devpage slide deck for one purpose and one audience — the story, the slides, the speaker notes — and iterate on it by looking at every slide. Use when asked to make, rewrite or improve a presentation, a talk, a pitch or a deck, or to turn a deck into a narrated video.
---

# Presentation

A deck exists to change what one audience knows or does. Decide that first;
every slide, word and picture serves it. Work in the operator's checkout or
the branch they name; asking for a deck authorizes writing it.

## 1. Purpose and audience, before any slide

Write these down (in chat, short) and get the operator's yes:

- **Who** is in the room: their job, what they already know, and their
  English — assume many are not native speakers.
- **What they must leave with:** one core message (12 words or fewer) and
  the three things they must remember.
- **What they should do after:** the one action (try it, approve it, fund it).
- **The mix:** how much is teaching, how much is selling.

## 2. The story, with other hats on

An engineer orders a deck by feature. That is a tour, not a story, and it
does not land. Before writing slides, get the order from people who are not
in dev mode: spawn two or three agents in parallel, each with one hat —

- a **product marketer** (launch storytelling: the core message, the order,
  what to cut);
- a **developer-relations / sales** person (the audience's objections and
  which slide answers each; where the "aha" lands; the first and last 20
  seconds word for word);
- for an internal deck, a **skeptical stakeholder** instead.

Give each the facts (point them at the feature catalog, not the code), the
audience and the constraints; ask for a slide-by-slide order with a headline,
a visual and one spoken point each. Then synthesize: keep what they agree
on, decide each disagreement and say why, and fact-check every claim they
make against the docs (they will get details wrong — and catch yours).

A shape that works for a product pitch:

1. The world the audience lives in now (they nod).
2. The pain in it (they feel it).
3. Why the usual fix fails.
4. The reveal: one command, one picture.
5. Two or three moments they *feel* — each one benefit, shown.
6. The frame or category ("an Office suite for developers") — only *after*
   they felt it, so it lands as earned, not as heavy.
7. Proof and reach (what else it does, how to share it).
8. One more thing — the surprise they'll repeat.
9. The action, as small as possible.

## 3. Slides: the speaker talks, the slide shows

- **A headline of six words or fewer and one picture.** No paragraphs, no
  bullet lists, no kicker labels. If a slide needs two headlines it is two
  slides.
- **The slide never says what the speaker says.** The headline is the claim;
  the notes carry the explanation in different words.
- **Show the moment, not the machine.** Draw what the user sees — a window,
  a before and after, a comment bubble — not the architecture. A diagram on a
  pitch slide has two or three boxes at most; never a sequence diagram.
  ("Is that marketing or an attempt to scare people?")
- **Fill the stage.** The stage is 1920 × 1080; a picture should be about
  1100–1500 px wide and text in it 28 px or more. Small, centered islands in
  white space read as unfinished.
- **True.** Every command, output, file and claim is real. Check details
  against the docs before saying them.
- **No idioms or jargon** on slides or in notes: no "single source of truth",
  "game-changer", internal names, protocol names.

In devpage, build the pictures as project widgets in the deck's `widgets`
folder, one file per picture, with shared pieces (a window frame, a sample
page) in a lowercase helper file. A `Slide` widget gives every slide the same
layout: a big centered headline, an optional line under it, and the picture
filling the rest. See [Making slides](../../../docs/boxes/viewer/making-slides/README.md).

## 4. Notes: the words the speaker says

Every slide gets `<Notes>` — the exact script, for a human presenter or for
`devpage video`:

- Two to four short sentences, 10 to 20 seconds spoken.
- Simple words, one idea per sentence, active voice, present tense.
- Talk to the listener ("your agent", "you"), like a person, not a manual.
- Lead into the next slide when it helps the story flow.

- Give each `<Notes>` a `delivery`: how the slide should feel ("honest and a
  little teasing", "relief and quiet confidence"). Across the deck the
  deliveries are the talk's emotional arc.

Put `audience`, `goal` and `tone` in the deck's yml, and `pronounce` lines for
words a voice gets wrong. They brief the narrator of `devpage video`; see
[what the narrator is told](../../../docs/boxes/viewer/making-a-video/README.md#what-the-narrator-is-told).

## 5. Iterate by looking

Never judge a deck from its source.

- Serve it and give the operator the link at once; they iterate with you.
- Look at every slide: `<deck>.yml/<n>.png` returns slide n as a PNG. Put all
  slides on one contact sheet to see the rhythm, then open each one full size.
  Check: is the headline readable from the back of the room, is the picture
  big enough, does anything overflow or look broken, in light and dark.
- A slide must not show its own deck's PNG live (that renders the deck from
  inside the deck); save the PNG as a file next to the slides and show that.
- `devpage check <deck>` must be clean.
- Fix, re-render, look again — until nothing on the sheet makes you wince.
- When it holds, `devpage video <deck>` makes the narrated version; watch it
  once start to finish.

## Done when

The operator agreed to the purpose and the story, every slide passes the
rules above when looked at, the notes read aloud cleanly, `devpage check` is
clean, and the showcase design (for devpage's own talk,
[the showcase](../../../docs/boxes/showcase/README.md)) describes the deck as
it now is.
