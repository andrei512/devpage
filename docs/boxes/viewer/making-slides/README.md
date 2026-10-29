# Making slides

How to make a deck: one Markdown file per slide, listed in order by a document
file of kind `slides`, shown one at a time on a 16:9 stage that prints to PDF
exactly as it shows. It is a task article under the [viewer](../README.md); the
document file in general is on [Writing a document](../writing-a-document/README.md).

## A deck

<FileTree>

- talks/launch/
  - devpage.yml — the deck: its title and the slides in order
  - intro.md — slide 1
  - problem.md — slide 2
  - demo.md — slide 3

</FileTree>

```yaml
kind: slides
title: Launch
slides:
  - intro.md
  - problem.md
  - demo.md
```

Each slide is its own Markdown file, so reordering a deck is moving a line in
its document file, and a slide can be reused in another deck. A slide is
ordinary Markdown — headings, lists, code, images, raw HTML and Mermaid
diagrams all work as on any page, with links and images relative to the
slide's own file.

**A real deck to copy.** [The devpage talk](../../../talks/devpage/devpage.yml)
is fourteen slides made this way, a talk about devpage made with devpage, with
speaker notes on every slide. Each slide is one headline and one picture: the
project widget `Slide` lays the slide out, and project widgets in
`docs/widgets/` draw the pictures — `Hero` for the title, `AgentFiles`,
`RawMarkdown`, `Scattered`, `CommandToPage`, `LiveSplit`, `CommentLoop`,
`Suite`, `AgentSees`, `ShareTiles`, `TalkFiles` and `Install`, sharing a
window frame from the helper `frames.tsx`. The deck names that folder with
`widgets: ../../widgets`. How to make a deck like it, from purpose and
audience to the last slide, is the `presentation` skill.

**Widgets on the stage.** A system widget on a slide looks as it does on a
page, magnified for the 1920 × 1080 stage; Mermaid diagrams fill the width
they are given.

## The stage

Every slide is laid out on a **1920 × 1080** stage and then scaled, as a
whole, to fit the window; the rest of the window takes the slide's own
background, so a slide fills the screen edge to edge. Text never
reflows when the window changes size: what fits on the stage fits everywhere,
and on paper. Write slides for that size; the stylesheet sets type large enough
to read from the back of a room.

<div style="max-width:30rem;margin:1.25rem auto;aspect-ratio:2/1;border:1px solid var(--border);border-radius:10px;background:var(--surface);display:flex;align-items:center;justify-content:center;position:relative;font-size:0.75rem;color:var(--muted)">
<span style="position:absolute;top:0.5rem;left:0.75rem">the window</span>
<div style="height:86%;aspect-ratio:16/9;background:var(--background);border:1px dashed var(--accent);border-radius:4px;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:0.4rem;color:var(--accent)">
<strong>1920 × 1080 stage</strong>
<span style="color:var(--muted)">scaled as a whole</span>
</div>
</div>

## Presenting

| Key | Does |
| --- | --- |
| <Keys>→ Space PgDn ↓</Keys> | next slide |
| <Keys>← PgUp ↑</Keys> | previous slide |
| <Keys>Home</Keys> / <Keys>End</Keys> | first / last slide |
| <Keys>F</Keys> | fullscreen on and off |
| <Keys>C</Keys> | the slide's comments on and off; nothing with comments off |

Keys pressed with <Keys>⌘</Keys>, <Keys>Ctrl</Keys> or <Keys>Alt</Keys> are left to the browser, so copying and
finding work as usual; F and C work in either case.

The slide's number is in the address — `/talks/launch/devpage.yml#3` is the
third slide — so a reload stays put and a link can point at one slide. A
change to a slide's file swaps it in place, on the slide you are on.

## On paper

`devpage pdf talks/launch` prints the deck one slide per page, each page the
stage's 16:9 shape, exactly as it shows ([Printing to PDF](../pdf/README.md)).
A deck has no header or footer settings: a slide is its whole page.

## Speaker notes

A slide's **notes** are the words a human presenter, or the narrator of a
recorded deck, says while the slide is up. They live in the
slide's own file, in a `<Notes>` tag, and are never shown on the slide — not on
screen, not on paper.

```markdown
# Why devpage

<Notes>
Every team writes docs, and every team's docs drift from the code.
devpage keeps them next to it.
</Notes>
```

`Notes` looks like a widget tag and is found by the same rules — inside code
and HTML comments it is just text — but it is devpage's own: a project widget
can't be named `Notes`. Its text is kept trimmed and as written, Markdown and
all. A slide has at most one `<Notes>`; a second one, `<Notes>` on a page
that isn't a slide, or a `<Notes>` without its `</Notes>` is a
[problem](../checking-links/README.md). An unclosed `<Notes>` runs to the end
of the file, so a typo like `</Note>` hides the rest of the slide rather than
showing its notes on it. A file counts
as a slide when it is shown in a deck, so the same file opened on its own is
a page and lists its `<Notes>` as a problem.

A `<Notes>` can carry a **delivery** — how the slide should feel when it is
said, as in `<Notes delivery="Curious, a little playful.">` — a direction for
the narrator, never said and never shown. It is a quoted string with no double
quotes inside; one that can't be read, or isn't a string, is a problem, and the
notes still never reach the slide. A deck says who it is for and how it
should sound in its own document file: its **audience**, its **goal** and its
**tone**, and `pronounce` lines for words a voice gets wrong. `devpage video`
records the deck with its notes read aloud
([Making a video](../making-a-video/README.md#what-the-narrator-is-told)).

```yaml
kind: slides
title: Launch
audience: Engineers who have never seen devpage.
goal: They see what devpage does and try it.
tone: Warm and plain, no hype.
slides: [intro.md, problem.md, demo.md]
```

## A slide as an image

Every slide is also a PNG: `<deck file>/<n>.png` — `/talks/launch/devpage.yml/3.png`
is the third slide, numbered from 1 as in `#3`. It is the slide as the browser
shows it on a 1920 × 1080 screen, with devpage's controls hidden. It is there
so whoever writes a slide's notes — an agent, usually — can look at the slide
they are writing for. A number the deck doesn't have, or a file that isn't a
deck, answers 404 with the reason.

**When it is taken.** Once the slide's widgets have mounted and every design on
the slide is painted — a design on another slide isn't shown, so it isn't
waited for, and a design with problems and nothing valid to draw counts once
its problems show. A slide that still isn't ready after 20 seconds is taken as
it is: the server logs what it waited for, and the PNG comes with
`X-Devpage-Ready: timeout`.

**One Chrome.** A running devpage starts one chrome-headless-shell for its
slides' PNGs, on the first one asked for. Each slide is shot in a tab of its
own, closed after, two at a time. Chrome is closed once it has drawn nothing
for a minute, and when devpage stops; the next PNG starts it again.

**Kept until a file changes.** A slide drawn once is kept, and asking again
answers at once; the PNG carries an `ETag`, so a request already holding it
gets `304 Not Modified`. Any change to a watched file forgets every kept
slide. A slide is shot without its comments — the deck is served to that
Chrome with comments off — so starting, replying to or resolving a thread
forgets none. At most 64 MB of PNGs are kept; the oldest go first.

**A slide showing a slide's PNG.** The tab Chrome shoots a slide in marks its
requests with `X-Devpage-Shot: 1`, and a slide's PNG asked for that way is
never drawn — it is the kept picture, or `508 Loop Detected` — so a slide
showing its own PNG, or one of another deck's, can't start shots without end.
`devpage pdf` marks its requests the same way. [`devpage video`](../making-a-video/README.md)
doesn't — its tab is recorded, not shot — so a slide's PNG on a recorded
slide is drawn for it.

## Comments and problems

Comments attach to the slide's file and show in a panel beside the stage when
you press <Keys>C</Keys> ([Comments](../comments/README.md)); they never appear in
fullscreen unless toggled, and never in a PDF or a slide's PNG. A missing slide file, or a
broken link on a slide, is listed above the stage while you write
([Checking links](../checking-links/README.md)).
