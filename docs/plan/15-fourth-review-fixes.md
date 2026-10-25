---
id: "15"
blocked-by: []
boxes: [viewer/designs, viewer/design-engine, viewer/checking-links, viewer/live-updates, engineering/sdlc, engineering/dependencies]
---

# Fourth drift review fixes

Findings:

- **A re-created design comes back.** A design deleted and written again —
  a branch switch, a stash, an agent rewriting a file — draws again in its
  canvas page and in every embed, without a reload. Test in Chrome.
- **A design's PNG address is a valid link.** `devpage check` and the
  preview accept `![…](x.design.png)` when `x.design` exists, and report the
  design's problems when it has some. A real file named `x.design.png` is
  not hidden: say which wins and make it so.
- **Release is described truthfully.** The engine crate is published with
  devpage at release, not already; the SDLC's Release section says the
  engine crate is published first, at the same version as devpage, and that
  the version moves past the published `0.1.0`. Bump both crates to the next
  version.
- **The CPU path reuses the cached layout** instead of laying out again on
  every paint, as the design-engine page says.
- **`docs/devpage.yml`'s watch list holds only what matches something.**
- **Raw `<img src="x.design">` embedding is taught** on the Designs page.
- **The Designs page's snippets are the example files**, or are labelled as
  simplified rather than as those files.

Notes that contradict a doc claim:

- Silently ignored values become problems: `width`/`height` `"fill"` on a
  root, `x`/`y` on a child of a frame with layout, negative sizes, a
  duplicated attribute.
- `devpage check` reports a drawing over the size limit, like the PNG route;
  the CPU canvas never stretches a large drawing.
- An import of a non-`.design` file says so, rather than "no such file".
- Hit-testing respects `clip`.
- An embed prints as it shows (fit to width and the window's height alike, or
  the doc says what print does).
- `devpage --help` names `.design`; `prost` only in dev-dependencies if only
  tests use it; the crate description mentions designs.

## Acceptance

- A test for each; vendored module rebuilt once at the end; gates green
  including three parallel `--ignored` runs and `cargo package`.
