---
id: "03"
blocked-by: ["01"]
boxes: [viewer/design-engine]
---

# Layout boundaries and runs

An edit to one item of a list costs the item, however many siblings it has.

Build:

- Boundaries: a fixed width and height stops an edit's layout at the node.
- Runs: an auto-layout frame with no `"fill"` child along its direction and
  no `"space-between"`; each child laid out on its own across the room the
  frame gives it; places from a prefix-sum tree of sizes and gaps (log time
  per change); a hugging run's length from the sums and breadth from a counted
  set of child breadths; `justify` center/end as one offset; rounding as
  taffy's pass rounds.
- Index: a run held once in the top level; its children found by binary search
  of the sums, widened by the largest overhang (counted set); a child's size
  change moves no entry.
- Damage of a run child's size change: one rectangle from its start to the
  run's end, before and after.
- Other frames keep taffy.

Acceptance:

- Layout of runs equals taffy's (a randomized test over row/column, align,
  justify, gaps, padding, hug/fixed sizes, nested runs, text that wraps).
- An edit that changes one item's size, and one that doesn't, in a column of
  1,000 / 10,000 / 100,000 items: update cost flat (measured before/after);
  tiles after the item repaint, tiles before don't; the drawing paints as a
  fresh one.
- Painting, hit testing and the view's tiles through a run match a fresh
  drawing's.
