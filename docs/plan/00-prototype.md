# Prototype: the tiled renderer

Measured: 10:48:42 → 11:12, 24 minutes of the 30. Gates green (fmt,
clippy `-D warnings` native and wasm32, `cargo test`). `cargo test --
--ignored`: 79 of 84 pass — the canvas on the GPU and on the CPU against the
PNG, font files on both, the zoom, embeds, slides on the CPU — and 5 fail:

- `images_larger_than_the_gpu_atlas…` and `a_navigation_from_a_full_atlas…`:
  on the GPU some tiles paint without their images (the screenshot shows
  whole tiles of the "many" design missing their 2000-px files) — the priming
  render isn't keeping the drawing's images resident for later tile renders,
  or vello re-packs between renders; to be found.
- `a_design_on_a_scaled_slide_is_painted_whole_on_the_gpu`: times out
  waiting for the scaled slide's design colour — the compositor's view under
  the stage's transform.
- `pdf::the_sidebar_stays_mounted…` and
  `comments_browser::selected_text_becomes_a_thread…`: outside anything
  this touched; to be checked against main.

One end-to-end path runs, in real code with tests, through every layer the
design moves: scene → chunks under property trees → two-level index → one
paint entry point → tiles → (a) the server's PNG streamed band by band
through the route, (b) GPU tiles composited by a textured-quad pass in the
canvas, (c) CPU tiles painted by `vello_cpu` in a worker and composited in a
2D canvas. An incremental update with damage runs in the engine.

## What it touched

- **Engine** (`design/src`): `display.rs` (new) — property-tree arenas
  (transform, clip, effect), commands carrying `(clip, effect)` state,
  `flatten` turning a node's nested Push/Pop (shadows) into effect/clip
  nodes, chunks with a per-chunk `rstar` index, the top-level `rstar`
  index (bulk-loaded; `take`/`put` for updates), `query` in paint order with
  chunk-level LOD, and `paint(list, area, scale, target)` — the layer-stack
  diff — behind a `Target` trait. `tiles.rs` (new) — tile rects, a CPU
  tile, the streaming PNG. `render.rs` — `chunked` (a fresh list), `own`
  (a node's commands in its own coordinates), the `Cpu` target,
  `paint_area`; `Live` + `Renderer::live`/`update` (the spike's incremental
  path); `Leaf::Text` now holds its text so a taffy tree can persist;
  the 16,384 check is gone from `drawing`. `rstar 0.13` added.
- **Module** (`design/wasm`): the `vello` `Target`; `Gpu::draw` is now a
  compositor (tile textures keyed by theme, level, column, row; nearest the
  middle first; a budget per frame; `settled` gates painting at a new zoom;
  a fresh renderer is primed with all the drawing's images in order);
  `Design::paint_tile` for the worker.
- **Page** (`core/assets`): `design-worker.js` (new built-in asset, in
  `build.rs`), the CPU compositor in `design.js` (tiles by
  theme:scale:col:row, a generation for staleness, settle delay), the GPU
  call with `settled` and a budget.
- **Server**: `Designs::png_to` writes band by band; the PNG route streams it
  as the body through a channel (problems known before the first byte).

## Where the difficulty actually was

- **Nowhere expected.** The property-tree painter (the layer-stack diff) and
  the tiled vello_cpu paint matched the whole-drawing paint on the four
  examples in both themes within 2/255 on the first run, shadows in pieces
  included; GPU tiles matched the PNG in Chrome on the first run.
- **A module worker drops messages that arrive during its top-level
  `await`** — the engine's load. Fixed by queueing them; worth a comment and
  a test that a canvas paints when its scene arrives before the worker is
  ready.
- **"Any size" has a time dimension.** `no_input_makes_the_engine_panic`
  painted PNGs of million-pixel drawings once the limit went: a
  1,000,000² PNG is 4 TB of pixels. The test now lays out, builds the list
  and paints one tile. The route stops painting when the client goes away
  (the channel's send fails).

## What the design got wrong or left open

- **A band's memory is the drawing's width × 256 rows.** Bounded for a tall
  design (400 px wide: 400 KB); for a 1,000,000-px-wide one, 1 GB. The design
  says "memory for one band"; whether a band should get shorter for very wide
  drawings is an open question for the operator.
- **Stable node ids**: the patch's ids are child-position paths. They are
  stable for every node outside a replaced subtree, which is enough for
  attribute edits and moves. A child inserted at the front of a long list
  shifts its siblings' ids, so they are all "replaced" and made again — the
  edit costs the list, not the node. Keyed ids would be a language/wire
  change; not in the approved design.
- **Blur as an effect**: the engine has no layer blur — shadows are sampled
  images — so effect nodes carry opacity and the additive blend only.
- The streaming PNG is always RGBA: an opaque drawing used to be written as
  RGB (vello_cpu's `into_png` scans first), which streaming can't know
  ahead. Decoded pixels are identical.

## What is left to be production-quality

- **Incremental update without O(n) walks** (`Live::update`): the spike
  diffs with `wire::diff`, rebuilds replaced taffy subtrees and uses taffy's
  caches, but then re-collects every hug, re-reads every frame and walks
  every node in `sync` (5 ms on 2,000 nodes). Needs: deeper diff inside a
  replaced subtree (only differing nodes dirty), hugs only on dirty paths,
  frames visited only along dirty paths and where a box changed, moved
  subtrees re-indexed, property-tree nodes of removed subtrees freed, the
  drawing's area kept from the chunks' bounds (today `list_for` still makes
  the whole old list to get it).
- **One display list, not two**: `Drawing` (the old nested list, used for
  the atlas's image order, `frames` and the wasm test) and `List` both exist;
  `drawing()` still builds the old list. The old `list`, `paint_list`,
  `Renderer::paint`, `display_list`, `MAX_SIDE` (still clamping
  `paint_list`), `scene::scene` (still used by the atlas tests) must go, the
  atlas's image order coming from the chunked build.
- **Damage → stale tiles**: computed by `Live::update` but not consumed. The
  browser marks *every* tile stale on any change (a generation); the server
  keeps no drawing and no tile cache at all (`png_to` builds a fresh list per
  request). Needs: the store keeping a `Live` per design per theme, updated by
  the patch, a capped LRU tile cache (64 MB) consulted by the band writer;
  the module keeping a `Live` and returning damage from `apply`, the
  compositors marking only overlapping tiles stale.
- **Tile caps**: neither compositor evicts; both need an LRU cap.
- **Hit testing** still walks the tree (`hit` in the module); move to the
  index (chunks need their node's box and interaction).
- **Text bounds** in the index are the line box inflated by one pixel;
  italic overhang or a font's glyph past its advance could be cut at a tile
  edge. Use the run's glyph bounds.
- **Tests the design names**: canvas paints only the tiles of its view on
  the 100,000-px design; pan cost independent of design size; GPU and CPU
  agree on a zoomed-in view; PNG with bounded memory (the store test pins
  writes under 1 MB each, not peak memory); an edit repaints only nearby
  tiles in the browser and the server. A CPU canvas painting before the
  worker is ready.
- **Docs**: engineering tests README (the too-big test is gone; new tests),
  the ETag still hashes the scene — fine.
