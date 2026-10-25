---
id: "01"
blocked-by: []
boxes: [viewer/designs, viewer/design-engine]
---

# The engine draws everything the design format promises

The engine parses, resolves, lays out and paints every element, attribute and
interaction [Designs](../boxes/viewer/designs/README.md) defines, and reports
every problem it lists with file, line and column.

- `Vector` (`d`, `viewBox`) and `Image` (`src`, PNG and JPEG, covering its box)
  painted; image bytes reach the engine through its file-reading input, inside
  the served folder only.
- Linear gradients, `textAlign`, opacity applied to a whole subtree, `clip`,
  per-corner `radius`, `strokeWidth` drawn inside the box.
- Every resolve problem from the Designs page, all reported at once; a design
  that uses itself is caught.
- Both backends — `vello_cpu` and the GPU `vello` translation — replay every
  display-list command; neither decides anything about the drawing.
- The example designs under `examples/checkout/` use every element and
  attribute, including an overlay design with a `close` button and a vector
  icon and an image, so each piece of surface has its consumer.

## Acceptance

- Unit tests: parsing (good files and each syntax error with its position),
  resolving (props, defaults, instance attributes, every problem kind, cycles),
  layout (hug, fill, gap, padding, align, justify, absolute), and painting the
  examples natively to a PNG that matches a committed reference within the
  tolerance the constraint names.
- Gates green apart from the pre-existing watcher tests.
