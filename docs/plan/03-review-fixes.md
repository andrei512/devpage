---
id: "03"
blocked-by: []
boxes: [viewer, engineering]
---

# Widgets drift review fixes

1. A `Tab` outside a `Tabs` is a problem (preview and `devpage check`).
2. An unknown capitalized tag is not passed through: it is replaced by an
   error placeholder naming it, and stays a problem.
3. A helper that doesn't compile is a problem with the compiler's message and
   line; a widget whose module fails to load shows that error in its place.
4. Configuration reference: the `widgets … is not a folder` error row (also
   for a folder outside the served folder); the example quotes
   `docs/devpage.yml` as it is.
5. Widgets styling truth: examples use real tokens (`--text`, not `--fg`) and
   real classes; drop the "theme's classes" bullet (the theme defines none for
   widgets) — widgets style with tokens, inline styles or their `css` export;
   no `vertical` prop on `Tabs`; the stylesheet uses only tokens that exist
   (`--font-mono`, not `--mono`/`--sans`).
6. Widget-folder problems are reported once per document, not once per page.
7. Mechanism docs: serving-pages' page frame (import map, widget list),
   live-updates' page script (mounts widgets), and a Widgets node in the viewer
   README's diagram with its click.
8. The widgets page shows live widgets beside their source (`Example`), so it
   shows rather than describes; "When a widget fails" says a render-time throw
   shows in the browser only and doesn't fail `devpage check`.
9. A Chrome test: a project `Callout.tsx` clash leaves the system `Callout`
   rendering.

## Acceptance criteria

- Tests for 1, 2, 3, 6, 9. Gates green incl. `--ignored`; `devpage check docs`
  exit 0; every diagram's clicks equal its links.
