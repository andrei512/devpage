---
id: "06"
blocked-by: []
boxes: [viewer/design-engine, viewer/live-updates, viewer/designs]
---

# Drift review fixes: the canvas and the stream

- A fix that returns a design to the last scene sent clears its problems:
  the stream tells the canvas the design is valid again even when no node
  changed, and the problems bar goes. A test saves a typo and then the
  original file, and finds the problems cleared.
- A design edit does refetch a page that embeds it — that is how its problems
  bar stays current — and the refetch keeps the page's canvases, their
  drawing and their streams. The live-updates page says exactly that instead
  of "a design edit never refetches the page".
- When the WebAssembly module fails to load, every canvas on the page says so
  in place of its drawing.
- A deleted design's canvas stops reopening its stream after `no such file`.
- The canvas page's title is the design's file name without `.design`, as the
  configuration reference says.
- `the_wheel_zooms_about_the_pointer_and_pans` waits for the paint it checks
  rather than a fixed sleep, and passes in the full parallel `--ignored` run.

## Acceptance

- The full `cargo test -p devpage -- --ignored` run is green three times in a
  row, in parallel.
