---
id: "12"
blocked-by: []
boxes: [viewer/designs, viewer/serving-pages, viewer/checking-links, viewer/pdf, engineering/dependencies]
---

# Second drift review fixes

- Embedding finds `<img>` elements properly, whether or not they end with
  `/>`: a raw `<img src="logo.png" width="40">` before an embed leaves the
  embed working, and a raw `<img src="x.design">` becomes an embed without
  swallowing what follows.
- The problems the Designs page shows are ones the code produces, and an
  unused import that doesn't exist is a problem, as the checking-links page
  says.
- The dependencies page lists every crate each workspace member declares.
- `#rgba` colours: documented, since the parser accepts them and a design may
  use them.
- A PNG asked for outside the served folder answers `404 … is not part of the
  served folder`, like every other path.
- The PDF page states how long `devpage pdf` waits for embedded designs and
  what it does when one never paints; the viewer page doesn't imply a
  design's canvas page takes comments.

## Acceptance

- Render tests for both `<img>` cases; a resolve test for the unused missing
  import; a route test for the 404.
