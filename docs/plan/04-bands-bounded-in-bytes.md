---
id: "04"
blocked-by: []
boxes: [viewer/design-engine]
---

# Bands bounded in bytes

A PNG's band holds at most 16 MB of pixels, whatever the drawing's width.

Build:

- Rows per band: as many as fit 16 MB, at least one. Up to 16,384 px wide, a
  whole row of tiles from the cache, as today; wider, shorter bands painted in
  pieces 256 wide through the one paint entry point, never cached.

Acceptance:

- A drawing 1,000,000 px wide (and a few rows tall) has a correct PNG (pixels
  checked at both ends and a seam), with what the band writer holds at once
  under 16 MB (a test bounding it) — and the numbers on main before.
- Narrow drawings unchanged: same tiles from the cache, same bytes.
