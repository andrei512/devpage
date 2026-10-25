---
id: "09"
blocked-by: []
boxes: [viewer/designs]
---

# A design is shown at its own size, never enlarged

As the [Designs](../boxes/viewer/designs/README.md#viewing-a-design) page now
says: on the canvas page and embedded in a page, a design is drawn at its own
size, scaled down only to fit — the page's width and the window's height for an
embed, the window for the canvas page — and never enlarged. An embed sits
centred on a quiet stage (theme tokens, light and dark) with a small link in
its corner that opens the design's canvas page. Zooming in stays available on
the canvas page. Painting stays crisp at the device pixel ratio.

## Acceptance

- Chrome tests: the 390-wide checkout design embedded in a doc page is 390
  CSS pixels wide, centred; in a 300-wide viewport it is scaled to fit; the
  button design on its canvas page is not enlarged; the stage's link opens
  the canvas page; clicking inside the embed still navigates.
- Checked by eye in light and dark, with a screenshot in the lane report.
