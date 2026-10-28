---
id: "01"
blocked-by: []
boxes: [viewer/design-engine, viewer/designs]
---

# Keyed node ids, from the parser to the drawing

Nodes have the identity the design docs give them, and every patch is keyed
by it.

Build:

- `key` on every element (unique among siblings, a problem otherwise).
- Element ids: a first parse numbers elements; `parse` against the last
  syntax tree matches by key, then Myers' diff over each sibling list by name
  and own attributes, then same-name leftovers in place; new elements take the
  file's next id. An engine API the store calls with the last parse.
- A node's id is the instance path of element ids; the wire carries it on
  every node.
- Patches keyed by id: changed (own attributes, no children), inserted (a
  subtree, after a sibling id or first, under a parent id), removed, moved.
  `wire::patch(old, new)` makes keyed ops by matching ids (still a whole-scene
  compare in this ticket — ticket 02 removes it from the server);
  `wire::apply` applies them all-or-nothing, as today.
- The drawing keyed by node id with order keys (fractional, between
  neighbours) for paint order; `Drawing::update` takes the keyed ops.
- The module applies keyed patches.

Acceptance:

- A random-edit test (inserts, removes, moves, attribute edits, at the front,
  middle and end, keyed and not) through parse → resolve → patch → apply →
  update: the drawing paints as a fresh one and every untouched node keeps its
  id.
- Inserting one row before the first of 1,000 / 10,000 / 100,000 rows: the
  patch holds one insert, the drawing remakes one chunk, and update cost is
  flat across the three (measured, with the numbers on main before this
  ticket for comparison).
- A duplicate key is a problem at the second.
