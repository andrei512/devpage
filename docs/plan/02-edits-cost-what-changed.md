---
id: "02"
blocked-by: ["01"]
boxes: [viewer/design-engine]
---

# An edit costs what it changed

`Drawing::update` does work proportional to the patch, never to the design.

Build:

- Inside each replaced subtree, compare old and new node by node; only nodes
  whose own attributes differ are dirty, and a subtree whose shape changed is
  rebuilt in the persistent taffy tree.
- Lay out from taffy's caches; re-hug only the no-layout hugging frames on a
  dirty node's path; visit afterwards only dirty paths, and a box's siblings
  only when that box's size changed.
- Remake only dirty chunks and chunks whose size changed; a node that moved
  gets a new transform offset and its subtree's chunks are moved in the index.
- Free the property-tree nodes and chunks of removed subtrees.
- Damage: each touched chunk's old and new bounds.

Acceptance:

- An edit to one row's text in the 100,000-px design damages only that row,
  and the tiles it doesn't overlap are pixel-identical to a fresh drawing's;
  tiles it does overlap are identical to a fresh drawing's too.
- Updating a row's text costs no more on a 10,000-row design than on a
  1,000-row one (within noise, a test that measures both); a row whose height
  changes moves the rows after it and re-indexes them, and the result paints
  as a fresh drawing does.
- Structural edits — a child added, removed, a subtree replaced, the root
  replaced — leave a drawing that paints as a fresh one (a randomized test
  over edits of a real design).
