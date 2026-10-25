# Vision

devpage renders the documents a software project lives by — system docs,
design docs, PRDs, slide decks — from Markdown files in the repository, live,
as you and your agents write them.

This page is the map of where devpage is going: **ideas, not commitments.**
Nothing on it is promised or scheduled, and most of it is not built. What runs
today is listed first, so the rest reads as direction rather than as a
description of the program.

## Built today

- A local program, built from source, that serves one document — a
  `devpage.yml` and the Markdown files in its folder — on `localhost`
  ([Installing devpage](installing.md)).
- GitHub-flavoured Markdown with raw HTML, rendered on every request, inside
  one fixed page frame shared by every page.
- Mermaid diagrams drawn as SVG in the browser, with clickable nodes, and
  server-side syntax highlighting of fenced code.
- A sidebar built from the pages' `## Topics` sections.
- Live updates: a watched file's change refreshes every open page in place.
- Light and dark themes from one set of color tokens, following the reader's
  system setting.

## Not built yet

- **No layouts.** There are no layouts per kind of document — no slide
  decks, no PRD or design-doc layouts; every page uses the same frame.
- **No extension API.** Markdown extensions, layouts and components can't be
  added from outside; the set devpage renders is fixed in its code.
- **No link checking.** A broken link renders as a link; a Topics link to a
  missing page is silently left out of the sidebar.
- **No reference extractors.** No page is generated from code.
- **No theme overrides in `devpage.yml`.** The file has only `title`, `root`
  and `watch`; the themes are built into the binary.
- **No static export.** Pages exist only while devpage is running.
- **No licence yet.** The repository has no `LICENSE` file, so "open source"
  below is an intention.

## Why

Software is designed in documents: what to build, how it works, why it was
decided. Today those documents are scattered across wikis, slide tools and doc
generators, each with its own format and its own copy, and the agents that now
write much of the code can read none of them where they live. Designing with an
agent then happens in chat, and chat is a narrow window: long text, no
structure, no pictures.

devpage puts the documents back in the repository, as the files everyone —
people and agents — reads and edits, and renders them well enough to think
with.

## What it is

- **Markdown in the repository is the source of truth.** There is no export,
  no second copy, no build step a person has to remember. The page in the
  browser is the file on disk.
- **Extended Markdown.** Standard Markdown, raw HTML anywhere, and extensions
  for what documents need: diagrams written as Mermaid and rendered as clean,
  themeable SVG, and further extensions as documents ask for them.
- **Realtime.** Pages update the moment a file changes, whoever changed it, so a
  design can be discussed while it is being written.
- **Layouts per kind of document.** A system doc, a design doc, a PRD and a
  slide deck share one source format; a document chooses its layout. A new kind
  of document is a new layout, not a new tool.
- **Designed to be read.** Typography, navigation and diagrams at the level of
  the best platform documentation, so understanding a system from its docs is
  faster than reading its code.
- **Fast and open.** A small Rust tool, open source, extensible by design rather
  than by forking.

## Inspirations

- **[DocC](https://www.swift.org/documentation/docc/)**, Apple's documentation
  compiler, sets the bar for how system documentation reads: typography and
  navigation good enough that understanding a system from its docs beats reading
  its code. devpage takes its best ideas — pages organize themselves by listing
  their children in a Topics section, every page leads with a summary, links are
  checked, and reference pages are generated from code by per-language
  extractors — without its limits: a fixed renderer, no custom components, no
  layouts beyond its own.
- **[The unreasonable effectiveness of HTML](https://claude.dev/blog/using-claude-code-the-unreasonable-effectiveness-of-html/)**
  argues that plain Markdown has become too thin for what agents now produce:
  past a hundred lines nobody reads it, and it can't hold diagrams, layouts or
  interactive controls. Rich, visual, interactive documents keep a person
  engaged with an agent's choices instead of handing them off. devpage keeps
  Markdown's strengths — plain files in the repository that people and agents
  diff and edit — and makes HTML a first-class part of it, so a document can be
  as rich as it needs to be without leaving the repository.

## Who it is for

Teams — of people and agents — who run their development process through
documents: write the design, agree on it, build it, keep the docs true. devpage
is where those documents are read.
