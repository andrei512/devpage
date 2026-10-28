---
id: "02"
blocked-by: []
boxes: [viewer/design-engine]
---

# Matching past the edit cap by a histogram diff

Past 256 edits in one sibling list, match by a histogram diff (anchors on
elements unique on each side, recursing between them), never by position.

Acceptance:

- 1,000 unkeyed rows with every third removed in one save: 333 removes and no
  change ops; every kept row keeps its id (before = main: ~667 changes).
- Rewritten lists (shuffled, reversed, half replaced) keep the ids of what
  survived; cost near-linear at 1k / 10k / 100k (measured).
- The `ids` module comment matches the code (elements compared whole).
