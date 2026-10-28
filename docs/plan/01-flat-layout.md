---
id: "01"
blocked-by: []
boxes: [viewer/design-engine]
---

# Every frame's edit costs what it changed

Build, per the design-engine README (a826c43) "Layout boundaries and runs":

- A run's new breadth reaches only children that stretch across it.
- A frame's own look (fill, stroke, radius, shadow, opacity, clip) reaches no
  child; opacity/clip property-tree nodes updated in place.
- Every auto-layout frame is a run: `"space-between"` as a counted gap in the
  sums; `"fill"` children resolved over the fill children only, against the
  free room from the sums, as taffy's flexbox resolves them (least sizes).
- A frame with no layout is a space: children laid out on their own, hug from
  counted sets of right/bottom extents, children indexed in frame coordinates.

Acceptance (release, at 1k / 10k / 100k siblings, before = main 5e09ab6):

- The reviewer's cases flat: the widest row's text made longer; the parent's
  fill changed; a no-layout frame's child resized and inserted at the front;
  a fill frame's non-fill child resized; a space-between frame's child resized.
- Randomized layout of all frame kinds equals taffy box for box (fill with
  least sizes, space-between, no-layout hug, nested), through a fresh layout
  and through updates; drawings equal fresh ones in pixels.
