# Making slides

How to make a deck: one Markdown file per slide, listed in order by a document
file of kind `slides`, shown one at a time on a 16:9 stage that prints to PDF
exactly as it shows. It is a task article under the [viewer](../README.md); the
document file in general is on [Writing a document](../writing-a-document/README.md).

## A deck

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

## The stage

Every slide is laid out on a **1920 × 1080** stage and then scaled, as a
whole, to fit the window, with the rest of the window left dark. Text never
reflows when the window changes size: what fits on the stage fits everywhere,
and on paper. Write slides for that size; the stylesheet sets type large enough
to read from the back of a room.

## Presenting

| Key | Does |
| --- | --- |
| → Space Page Down ↓ | next slide |
| ← Page Up ↑ | previous slide |
| Home / End | first / last slide |
| F | fullscreen on and off |
| C | the slide's comments on and off |

Keys pressed with ⌘, Ctrl or Alt are left to the browser, so copying and
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
you press **C** ([Comments](../comments/README.md)); they never appear in
fullscreen unless toggled, and never in a PDF. A missing slide file, or a
broken link on a slide, is listed above the stage while you write
([Checking links](../checking-links/README.md)).
