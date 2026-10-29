# Checking links

How devpage finds broken links: what counts as a problem, how the preview shows
problems while you write, and how `devpage check` fails a test or a CI job on
them. It is a task article under the [viewer](../README.md). Links are most of
what it checks, so the page keeps their name; a diagram that doesn't parse, a
design with problems and a misplaced `<Notes>` are found and reported the same
way.

## What is checked

Every link a reader could follow and every file a page loads — Markdown links,
images, links and `src` attributes in raw HTML, and the targets of Mermaid
`click` lines:

| What | Must |
| --- | --- |
| A target inside the served folder | exist, inside the [served folder](../constraints/the-served-folder-is-the-boundary.md) |
| A `#fragment` | name a heading or id the page it points at renders — in a doc of several chapters, `<chapter>--<id>` or the chapter's own name; see [Headings](#headings) |
| `#n` on a deck | be a slide the deck has |
| A document file | be valid for its kind, with its `root`, `chapters` and `slides` naming existing files |
| A Topics entry | point at an existing page |
| An `http` or `https` address | answer with a status below 400, following redirects |
| A comment thread | parse, for each file in the comment store |
| A design a page links to or embeds | parse and resolve, with every design it imports — see [Designs](#designs) |
| A Mermaid diagram | parse — see [Diagrams](#diagrams) |

An external answer of 400 or above is a broken link at once. An address that
doesn't answer within 10 seconds, or can't be connected to, may only be a slow
network, so it is asked again before it counts — see
[In the preview](#in-the-preview) and [`devpage check`](#devpage-check) for
how. devpage asks for each one as a browser asks for a page, accepting HTML,
since some sites answer other requests with an error no reader sees. Each
address is asked once per check, however many links point at it.

`mailto:` and other schemes are not checked. A scheme is what comes before a
`:` that precedes any `/`, `?` or `#`, so `notes/a:b.md` and `b.md#at:1` are
files like any other.

Links in a [widget](../widgets/README.md)'s content are checked like any other,
since devpage renders that Markdown. Links a widget's own code builds in the
browser are not: devpage never runs widget code on the server.

### Headings

A `#fragment` is checked against the page it points at as that page shows
it: its content rendered, the tags of its [widgets](../widgets/README.md)
expanded — the widgets of the document that shows it at its own address: its
nearest site, or, in no site, the page alone; a doc's own, for a doc. So a
heading inside a widget can be linked to — `<Steps>` around `## One` renders
`#one`, and `page.md#one` passes — and a heading inside a tag that is no
widget can't: the page shows only the tag's error in its place. These are
the ids [search](../search/README.md#what-is-indexed) gives its sections, so
every section it finds is somewhere a link can land.

### Designs

Every `.design` file a page links to or embeds, and every design those
import, must parse and resolve: known elements, attributes and props, values
of the right type, imports and images that exist in the served folder, and no
design that uses itself ([Designs](../designs/README.md#problems)). They are
read from the page's Markdown — an image or a link whose target ends in
`.design`, never an example in code — and each of the design's problems is
reported on the page as
`broken design <target> — <file>:<line>:<column>: <message>`. A design whose
images don't fit the GPU's atlas together is a problem too. A link to a design's PNG
address, `x.design.png`, is checked as the design `x.design`
([Looking at a design as a PNG](../designs/README.md#looking-at-a-design-as-a-png)).

### Diagrams

Every `mermaid` fence on a page must be a diagram pele can draw. The server
draws it while it renders the page, so the check sees exactly what a reader
would ([Diagrams](../diagrams/README.md#checking-diagrams-before-committing));
one that doesn't parse, or hits the engine's time or memory limit, is
reported at its fence's line as `broken diagram — <pele's message>`, and the
page shows the [error block](../diagrams/README.md#when-a-diagram-fails)
where the diagram would be. The targets of a diagram's `click` lines are
checked as links once it is drawn.

## In the preview

While devpage serves a document, each page lists its own problems in a bar at
its top, and every broken link on it is marked where it stands — a red wavy
underline on a link, a dashed outline on an image, a dashed red border on a
diagram node. They update as
you edit, like the rest of the page: fix the link, save, and the problem goes.
The problems are found in the very content the page shows: devpage renders a
page's content once, checks it, and puts the frame round it — so a link of the
frame's own, the navbar's or the sidebar's, is checked with the document file
rather than as a link on the page.
devpage also prints each problem once on its terminal, as it first finds it:

<Terminal title="devpage's terminal">

```text
$ devpage docs
devpage → http://localhost:3030
docs/boxes/viewer/README.md:42: broken link ../serving/README.md — no such file
docs/README.md:7: broken link design/devpage.yml — in docs/design/devpage.yml: chapter rollout.md does not exist
docs/README.md:9: broken link talk/devpage.yml#9 — the deck has 3 slides
docs/README.md:12: https://example.com/gone — 404 Not Found
docs/planting.md:14: broken diagram — No diagram type detected for the given text.
```

</Terminal>

Files are named by their path from the top of the served folder. A document
file that can't be loaded is reported on the page that links to it.

External links are checked in the background, so a slow site never holds a
page up. An address that answers 400 or above shows as a problem as soon as
its answer arrives. One that doesn't answer within 10 seconds, or can't be
connected to, is asked again after 30 seconds, then 60, then every 120 — and
shows as a problem only after three failures in a row, as
`<address> — <why>, 3 times in a row`. Once it answers, the problem goes. An
address that has answered is not asked again for as long as devpage runs.
Each change reaches open pages through the
[live-update stream](../live-updates/README.md), like a saved file.

On a [server](../server/README.md), a published version never changes, so a
page's problems are found on its first view and kept: every later view shows
the same bar without checking again
([what a version holds in memory](../server/storage/README.md#what-a-version-holds-in-memory)).
External links are not asked there at all — the CLI asked them before the
upload.

## `devpage check`

<Terminal>

```
$ devpage check docs
docs/README.md:12: https://example.com/gone — 404 Not Found
$ echo $?
1
```

</Terminal>

It takes a document file's folder (`docs`, for `docs/devpage.yml`), one
Markdown file (`devpage check design.md`), or one design
(`devpage check app/checkout.design`), whose problems it prints as
`<file>:<line>:<column>: <message>`.

`devpage check` takes the same argument as `devpage`, checks that document and
every page and document reachable from it by links within the served folder,
prints every problem in the same `file:line: problem` form, and exits with `1`
if there was any and `0` if there were none. An external address that doesn't
answer within 10 seconds, or can't be connected to, is asked once more with 30
seconds to answer before it is reported; one that answers 400 or above is
reported at once. It starts no server. Use it in a
test or a CI job to keep the docs from shipping a broken link or a diagram
that doesn't parse. devpage's own
tests check devpage's docs the same way, external links aside, so the test
suite needs no network.
