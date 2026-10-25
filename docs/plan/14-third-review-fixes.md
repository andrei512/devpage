---
id: "14"
blocked-by: []
boxes: [viewer/designs, viewer/design-engine, viewer/constraints, engineering/developing-devpage]
---

# Third drift review fixes

- **The engine never panics on input.** A colour with non-ASCII characters (a
  pasted zero-width space) is a problem, not a panic; audit every byte-sliced
  string in the parser and resolver. The server and the watcher survive any
  design file. Test: hostile inputs — non-ASCII everywhere, truncated files,
  huge numbers, deep nesting — through parse, resolve, layout and paint, and
  through a running store, which keeps answering afterwards.
- **`hug` and `fill` under `layout="none"`, as in Figma:** a hugging frame with
  no layout takes the extent of its children (their `x` + width and `y` +
  height); `"fill"` on a child of a frame with no layout is a problem at the
  attribute.
- **`props` on any element but the root is a problem.**
- **The parser reads JSX's own rules, documented:** double- or single-quoted
  strings with no backslash escapes (JSX has none), a bare attribute meaning
  `{true}`, `-` allowed in names, booleans `{true}` / `{false}` in the value
  grammar. The Designs and design-engine pages say exactly what is read.
- **The whole drawing everywhere** (the Designs page's "drawing"): the engine
  computes a design's painted bounds; the PNG, the CPU canvas and the GPU
  canvas all show exactly that area, and the canvas page and embeds fit it.
  The one-renderer Chrome test compares the whole drawing, including a design
  with a root shadow.
- The developing page's stale "untested" claim about problems on the canvas
  goes; counts updated.
- Rebuild the vendored module once, at the end.

## Acceptance

- Tests for each bullet; the GPU-vs-PNG test fails today on `button.design`'s
  shadow and passes after.
