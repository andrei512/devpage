---
id: "01"
blocked-by: []
boxes: [viewer/design-engine]
---

# One chunked display list, painted and hit-tested through the index

The engine has one display list: the chunked one under property trees, with
its index. `Drawing` *is* that list — kept, updatable — and everything that
makes pixels or answers a click goes through it.

Build:

- `Renderer::drawing` makes the chunked list directly (no nested list made
  first): the drawing's area kept from the chunks' bounds as they are put and
  taken (an extent that knows its min and max without a walk), the root's box
  always in it; the images in the order the chunked build first paints them,
  for the atlas check and the GPU's priming; the blob reuse from `after` kept.
- `Drawing::update(renderer, old_scene, new_scene) -> Damage` exists with the
  prototype's semantics (`Live::update`), so lanes downstream can call it;
  ticket 02 makes it cost only what changed.
- A command's index bounds cover every pixel it can touch: glyph runs from
  their glyphs' bounds, not the line box.
- Hit testing through the index: the chunks at the point in paint order, the
  topmost whose node has an interaction, whose box holds the point and whose
  clipping frames show it. The module's tree walk goes.
- The tile cache, in `tiles`: keyed by theme, scale, column and row, capped
  by a cost (bytes, or a count), least recently used dropped first, with
  stale marking by damage rects; stale entries are still returned, as stale,
  until replaced. Generic over what a tile holds, so the server's pixels and
  the module's GPU textures use the same type.
- Contract: delete the old path — the nested `list`, `paint_list`,
  `Renderer::paint`, `display_list`, `MAX_SIDE`, `scene::scene` — and move
  every caller and test (the module's atlas tests included) to the chunked
  list and the one paint entry point. `Live`/`chunked` fold into `Drawing`.

Acceptance:

- No `Push*`/`Pop` survives in what a drawing holds; `Draw` has only drawing
  variants, a shadow builds its layers as property-tree nodes.
- The examples, every shadow-piece test and the font tests pass through tiles,
  within the engine's tolerance and at most 2/255 of the old whole paint.
- A click on the 100,000-px design is answered from the index (a test
  asserting the right action at the bottom row, and none outside a clip).
- The cache: a test that the cap holds, the least recently used goes first,
  and damage marks exactly the overlapping tiles stale at every scale.
- `cargo test`, clippy native and wasm32, the module rebuilt; the Chrome
  canvas tests that passed in the prototype still pass.
- The design-engine docs' sentences about the engine's internals match.
