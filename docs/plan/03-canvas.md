---
id: "03"
blocked-by: []
boxes: [viewer/designs, viewer/design-engine, viewer/serving-pages, viewer/pdf]
---

# The canvas is a full prototype viewer, and pages embed designs

- The WebAssembly module uses wasm-bindgen for every export and holds one
  scene per canvas, so a page can hold several designs.
- The canvas page fits the design to the window; zoom with pinch or
  Ctrl/⌘ and the wheel, pan by dragging; paints only when the scene or the view
  changes.
- Interactions: `navigate` (history push), `back`, `overlay` (centered over a
  dimmed backdrop, its own stream) and `close`; the pointer is a hand over an
  interactive node.
- Problems shown over the last valid drawing; the stream reopened after devpage
  restarts; a hidden tab closes its streams.
- `![Name](x.design)` in Markdown renders an embedded, clickable canvas at the
  page width; a page refetch leaves its canvases alone; `devpage pdf` waits for
  embedded designs to paint, so they print as they show.
- The module is built in release with size optimisation.

## Acceptance

- Chrome tests (`--ignored`): the canvas paints on WebGPU and on the CPU
  fallback, and the two are within the constraint's tolerance of the server's
  PNG; clicking Pay navigates and back returns; an overlay opens and closes; an
  embedded design in a page draws and survives a page edit; a doc with an
  embedded design prints it.
