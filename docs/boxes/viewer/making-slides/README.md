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
is fourteen slides made this way, a pitch for devpage made with devpage. Every slide is a
[widget](../widgets/README.md) or two: devpage's own (`Terminal`, `FileTree`,
`Example`, `Keys`, Mermaid diagrams) and the project widgets in `docs/widgets/`
that lay a slide out — `Hero` for a title, `Columns` for side-by-side blocks
(or rows), `Scattered`, `Chips`, `PaperSketch` and `FrameSketch` for its
pictures. The deck names that folder with `widgets: ../../widgets`.

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
| <Keys>C</Keys> | the slide's comments on and off |

Keys pressed with <Keys>⌘</Keys>, <Keys>Ctrl</Keys> or <Keys>Alt</Keys> are left to the browser, so copying and
finding work as usual; F and C work in either case.

The slide's number is in the address — `/talks/launch/devpage.yml#3` is the
third slide — so a reload stays put and a link can point at one slide. A
change to a slide's file swaps it in place, on the slide you are on.

## On paper

`devpage pdf talks/launch` prints the deck one slide per page, each page the
stage's 16:9 shape, exactly as it shows ([Printing to PDF](../pdf/README.md)).
A deck has no header or footer settings: a slide is its whole page.

## Comments and problems

Comments attach to the slide's file and show in a panel beside the stage when
you press <Keys>C</Keys> ([Comments](../comments/README.md)); they never appear in
fullscreen unless toggled, and never in a PDF. A missing slide file, or a
broken link on a slide, is listed above the stage while you write
([Checking links](../checking-links/README.md)).
