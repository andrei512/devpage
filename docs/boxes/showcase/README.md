# The showcase deck

The design of devpage's own presentation, `docs/talks/devpage/`: what it is
for, the slides it has and what each one proves, and the rules every slide
follows. The deck is devpage demonstrating itself, so it is held to the same
bar as the code: when devpage gains a capability a user sees, the deck shows
it ([the rule](../../constraints/features-arrive-on-the-deck.md)). It belongs
to the [system catalog](../README.md).

## Four examples, one per kind

The deck is one of four documents devpage keeps as live examples of its
kinds, all real and all in this repository, reached from the navbar's
**Examples** menu:

| Kind | Example | Why it is real |
| --- | --- | --- |
| site | these docs, `docs/devpage.yml` | devpage's documentation |
| doc | the feature catalog, `docs/features/devpage.yml` | a quick tour of every feature, one short chapter per area, printable |
| slides | this deck, `docs/talks/devpage/devpage.yml` | devpage's presentation |
| design | the checkout prototype, `docs/designs/checkout/checkout.design` | the designs these docs, the catalog and this deck embed, using every element and interaction a design has |

## What it is for

A developer who has never seen devpage watches the talk — live, as a PDF or as
its narrated video — and leaves knowing three things: their agent already
writes their documents, devpage makes those files easy to read, review and
share, and it starts with one command. It is half lesson, half pitch, for an
audience of which many are not native English speakers. The feature catalog
doc explains each feature; the docs site teaches them.

The core message: **your documents live with your code — now they are easy to
read.** The frame comes late, once the audience has felt it: devpage is an
Office suite for developers.

## The slides

A story in order, not a feature tour: the world the audience lives in, the
pain in it, the reveal, the moments they feel, the frame, the proof, the
action. Each row is one slide.

| # | Headline | The picture | The point the notes make |
| --- | --- | --- | --- |
| 1 | devpage | the logo | live documents for you and your agent |
| 2 | Your agent writes a lot. | a repository's files, most marked "written by your agent" | designs, plans and reviews are Markdown in the repo now |
| 3 | Nobody reads raw Markdown. | that design as raw text in a dark editor | a diagram is code, a table is lines, so we skip it |
| 4 | So we copy it out. Then it goes stale. | a wiki, a deck, a document and a chat, scattered, two stamped "out of date" | copies drift and the agent can't see them |
| 5 | One command. | `devpage docs` beside the same design rendered as a page | every file is a page, nothing copied |
| 6 | Save. It's there. | the agent's editor with a new line, the page showing it | pages update as the agent writes |
| 7 | Comment like Google Docs. | a highlighted phrase and a comment; read → fixed → closed | review goes straight to the agent |
| 8 | An Office suite for developers. | four tiles: site, doc, slides, design, each "like" its office app | every kind of document, as files |
| 9 | Designs, as code. | one real line of `checkout.design` beside the live prototype, in its own fonts, light or dark with the deck | screens the agent writes and you click |
| 10 | Your agent sees its work. | slide 5 as a PNG, and the agent saying what it fixed | every slide and design is an image; links and diagrams are checked |
| 11 | Share it anywhere. | PDF, video and team-site tiles | people outside the repo get it too |
| 12 | One more thing. | the deck's folder and `devpage video` | this talk is files, and so is its video |
| 13 | Try it today. | the two commands | the action |

## Rules for every slide

- **One headline, one picture.** A headline of six words or fewer and one
  picture filling the stage; no paragraphs, no bullet lists. If a slide needs
  two headlines it is two slides.
- **The speaker talks, the slide shows.** Every slide has `<Notes>` with the
  exact words the speaker (or the narrator of `devpage video`) says, in
  different words from the slide: two to four short sentences in plain
  English, no idioms, and a `delivery` for how the slide should feel. The deck's
  `audience`, `goal` and `tone` brief the narrator.
- **Show the moment, not the machine.** Pictures are what the user sees — a
  window, a page, a comment — drawn by the project widgets in
  `docs/widgets/` (`Slide` lays every slide out). A diagram has two or three
  boxes at most.
- **Fits the stage.** Nothing scrolls or overflows at 1920 × 1080, and every
  slide reads in light and dark and in the PDF.
- **True.** Commands, output, file names and claims are real.

How a deck like this is made, step by step, is the `presentation` skill.

## Constraints

- [A feature a user sees arrives on the deck](../../constraints/features-arrive-on-the-deck.md)

## Topics

### Constraints

- [A feature a user sees arrives on the deck](../../constraints/features-arrive-on-the-deck.md)
  — the change that adds it adds or remakes its slide.
