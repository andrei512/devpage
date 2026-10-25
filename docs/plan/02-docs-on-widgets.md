---
id: "02"
blocked-by: []
boxes: [viewer, engineering]
---

# devpage's docs use widgets

Refactor devpage's own docs so they show rather than describe, using the
system widgets and the KindCards project widget:

- `Example` — Markdown source beside its result: writing-a-document (the
  Markdown that works), writing-a-doc, making-slides, widgets.
- `FileTree` — folder layouts: getting-started (the garden), comments (the
  comment store), deploying (`/srv`), writing-a-doc/making-slides layouts.
- `Tabs` — near-identical variants: configuration-reference (site / doc /
  slides), installing (crates.io / from a checkout), writing-a-document's three
  document files.
- `Terminal` — commands and real output: checking-links, installing,
  getting-started's run steps.
- `Callout` — warnings that mustn't be missed: deploying (only through the
  proxy), pdf (first render only), widgets (one name, one widget), the served
  folder boundary.
- `Keys` — making-slides' key table.
- `KindCards` — the landing page and writing-a-document's three kinds.

Change presentation only: every claim stays as it is, every link and anchor
still resolves, every diagram still renders with its clicks.

## Acceptance criteria

- Each page listed uses the widgets named for it, and reads well in the
  browser in light and dark (checked in headless Chrome screenshots).
- `devpage check docs` exit 0 and the docs-check test pass.
