---
id: "03"
blocked-by: ["01"]
boxes: [viewer/design-engine, viewer/designs]
---

# The server keeps drawings and tiles

The store keeps, with each design's scene, its drawing in each theme it
shows in, updated by the patch from its last scene; its tiles at 1× in a
cache shared by the store, capped at 64 MB of pixels, least recently used
dropped first; damage marks only overlapping tiles stale. The PNG is
assembled band by band from that cache, painting only missing or stale
tiles, into the streaming encoder the route sends as its body.

Build:

- The store's tiles in the engine's tile cache (ticket 01), capped at 64 MB.
- The store's `drawable` check reads the kept drawings; a resolve updates
  them with `Drawing::update` instead of laying out afresh.
- `png_to` writes from the kept drawing and the cache; `png` (whole bytes) is
  only what tests need, or goes.
- The route keeps streaming; a client that goes away stops the painting.

Acceptance:

- A PNG asked for twice paints no tile the second time; after an edit to one
  node of the 100,000-px design it paints only the tiles over the damage (a
  test counting tiles painted).
- The PNG of the 100,000-px design is made holding one band plus the capped
  cache: a test bounding what the writer holds at once, and the cache never
  past its cap.
- The ETag and `304` behaviour unchanged; problems still `422` before any
  byte.
- The engineering tests list and the designs docs match.
