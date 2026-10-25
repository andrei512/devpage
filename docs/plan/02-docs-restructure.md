---
id: "02"
blocked-by: ["01"]
boxes: [viewer, engineering]
---

# Restructure devpage's docs: shorter pages, more visual

Presentation and structure only — no claim changes; every link and anchor
still resolves (old anchors moved to a new page get their incoming links
updated). No screenshots of devpage. Don't touch the deck (`docs/talks/`).

1. Wide diagrams (viewer landing, serving pages, SDLC stages) are kept to about
   six nodes; split big ones, or replace with `Cards` plus a small diagram.
2. Landing pages (root, catalog, viewer, engineering) stop saying everything
   twice: `Cards` instead of bullet lists repeating the diagram. The viewer's
   glossary moves to its own reference page; its constraints listed once.
3. Splits, each page one job, Topics/sidebar updated:
   - Widgets → Widgets · Using widgets · System widgets (one live `Example` per
     widget) · Writing a project widget · How widgets run.
   - SDLC → SDLC · Stages · Branches · Review bars and CI ("who starts a stage"
     lives only on Skills reference).
   - Writing a document → Writing a document · Links between pages and
     documents · Markdown that works.
   - Serving pages → Serving pages · Which document a file belongs to ·
     Rendering a page.
   - Developing devpage → Developing devpage · Tests · Code map.
   - Diagrams and themes → Diagrams · Themes (with color swatches) · How themes
     reach code and diagrams (pele's variables live only here).
   - Getting started → Getting started (ideas as `Cards`, one connected
     diagram) · Your first site, doc and deck (`Steps`, `FileTree`, `Tabs`,
     `Terminal`).
   - Live updates → Live updates · The page script.
   - Comments → Comments · The comment store.
4. One home per fact: the five-layer table, the flags table, "who starts a
   stage", the constraint lists, pele's variables, "how these docs are
   organized".
5. Smaller pages: commands with output → `Terminal`; folder layouts →
   `FileTree`; multi-step rules → decision flowcharts; procedures → `Steps`;
   constraint pages share one shape (rule in a `Callout`, then Why, Check).
   The configuration reference stays one page (layout fixes only).

## Acceptance criteria

- Every page renders well in light and dark (checked in headless Chrome).
- `devpage check docs` clean on internal links; every diagram's clicks equal
  its links; the docs-check test passes; gates green.
