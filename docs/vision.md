# devpage

devpage renders the documents a software project lives by — system docs,
design docs, PRDs, slide decks — from Markdown files in the repository, live,
as you and your agents write them.

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

## Who it is for

Teams — of people and agents — who run their development process through
documents: write the design, agree on it, build it, keep the docs true. devpage
is where those documents are read.
