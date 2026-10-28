---
id: "04"
blocked-by: ["01"]
boxes: [viewer/design-engine]
---

# The compositor, on the GPU and the CPU

The canvas is drawn by the compositor of the design docs, on both backends.

Build:

- GPU: tiles in the engine's tile cache (ticket 01), capped (by count, above what a 4K view at device
  pixel ratio 2 needs); damage from each applied update marks only the
  overlapping tiles stale, shown until replaced; the priming render keeps the
  drawing's images in the atlas so every tile paints its images (the
  prototype's `images_larger_than_the_gpu_atlas…` and
  `a_navigation_from_a_full_atlas…` failures); a design on a scaled slide
  paints whole (`a_design_on_a_scaled_slide_is_painted_whole_on_the_gpu`).
- CPU: the worker's tiles in a capped LRU in the page; damage from the
  page's own copy of the design marks only overlapping tiles stale; a canvas
  whose scene arrives before the worker has loaded still paints (the worker
  queues early messages).
- `data-painted` means every tile of the view is current at the view's own
  scale; the canvas records how many tiles it has painted, for the tests.

Acceptance (Chrome, `cargo test -- --ignored`):

- The 100,000-px design on its canvas page paints only the tiles of its view.
- A pan's cost doesn't grow with the design: frames of a pan over cached
  tiles on the huge design take no longer than on a small one (measured), and
  paint no tile.
- GPU and CPU agree on a zoomed-in view (4× into the checkout) by the
  engine's rule, and both are sharp there: each compared with the engine
  painting that area at that scale with `vello_cpu`.
- An edit to one node repaints only the tiles around it, on the GPU and on
  the CPU.
- Every Chrome test passes; the engineering tests list matches.
