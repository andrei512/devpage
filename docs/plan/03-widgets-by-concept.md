---
id: "03"
blocked-by: []
boxes: [viewer/widgets, viewer/widgets/how-widgets-run]
---

# Widgets split by concept, caches owned

The widget module becomes one module per concept: the widget set a document
has, the Markdown widget tags (pure text in, text out — what remains after the
prototype moved the scanner to markup), TSX compilation, and the module cache
with its versioning. The process-wide read and compile caches become values
owned by whoever holds the document, invalidated through that owner rather
than by the watcher reaching into a global.

## Acceptance

- No global cache in the widget modules.
- The Markdown widget-tag preprocessing is tested on strings alone.
- Each new file holds one concept; none is a pass-through (deletion test).
- Widget pages, the load set and content addresses unchanged; gates green
  including `cargo test -- --ignored`.
