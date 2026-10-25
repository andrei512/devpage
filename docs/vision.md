# Vision

devpage renders the documents a software project lives by — system docs,
design docs, PRDs, slide decks — from Markdown files in the repository, live,
as you and your agents write them.

This page is the map of where devpage is going: **ideas, not commitments.**
Nothing on it is promised or scheduled, and most of it is not built. What runs
today is listed first, so the rest reads as direction rather than as a
description of the program.

## Built today

- Three kinds of document from Markdown in the repository: a **site** with a
  sidebar built from Topics sections, a **doc** of chapters with an outline,
  and a deck of **slides**.
- GitHub-flavoured Markdown with raw HTML, server-side syntax highlighting, and
  Mermaid diagrams drawn as SVG with clickable nodes.
- Live updates: a file's change refreshes every open page in place.
- Comments on text or paragraphs, kept as files outside git, so people and
  agents iterate on a design through the docs themselves.
- Checked links, in the preview and as `devpage check`.
- PDF printing of a doc or a deck, identical to the screen.
- Light and dark themes from one set of color tokens.
- Installed with `cargo install devpage`; run locally, or deployed for a team
  behind an auth proxy. MIT licensed.

## Not built yet

- **No extension API.** Markdown extensions, layouts and components can't be
  added from outside; the set devpage renders is fixed in its code.
- **No reference extractors.** No page is generated from code.
- **No theme overrides.** The themes are built into the binary.
- **No static export.** Pages exist only while devpage is running.

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
  slide deck share one source format; a document chooses its kind. A new kind
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
