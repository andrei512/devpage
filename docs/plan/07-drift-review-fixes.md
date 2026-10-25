---
id: "07"
blocked-by: []
boxes: [viewer, engineering]
---

# Drift review fixes

The design-branch review found code and docs that disagree. Settled answers:

1. **`watch` belongs to its site.** A site's `watch` list narrows refreshes for
   paths inside that site's folder only, whichever document devpage was started
   with; the nearest site's list applies. Paths in no site with a `watch` list
   follow the default (everything not excluded by `.gitignore`). Document files
   and the comment store always refresh.
2. **`#n` on a deck is checked**: `n` must be between 1 and the deck's slide
   count, in the preview and in `devpage check`.
3. **A site's title links to that site's root page**, not `/`.
4. **The example image serves devpage's docs with no broken links**: copy
   `LICENSE` and `examples/` beside `docs/`; the deploying page shows the real
   Dockerfile (`rust:1.89-slim-bookworm`).
5. **Getting started** links the deck from the garden's `README.md` too, so
   `devpage check garden` covers all three as it says.
6. **`?print`** is described as applying to document files and Markdown files
   outside any site (a site is never printed).
7. Small text: the developing-devpage test count, the checking-links example
   output (show the real form, with paths relative to the served folder — make
   the code print served-folder-relative paths, not absolute ones), the
   live-updates diagram's message.

## Acceptance criteria

- Tests: a second site's `watch` narrows its own folder and doesn't affect the
  entry's pages, under either entry; `#9` on a 2-slide deck is a problem; a
  second site's title links to its root.
- `devpage check docs` passes in a copy laid out like the image (`/srv` with
  `docs`, `examples`, `LICENSE`).
- Every claim the review quoted now matches the code; gates green.
