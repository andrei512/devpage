---
id: "07"
blocked-by: []
boxes: [viewer/designs, viewer/design-engine, viewer/constraints, engineering/developing-devpage]
---

# Drift review fixes: the engine's rules and its one-renderer check

- An instance takes every attribute every element takes — `name`, `x`, `y`,
  `width`, `height`, `opacity`, `onClick` — plus the design's props; the
  designs page says that in one place, consistently.
- `opacity` outside `0` to `1` is a problem.
- The resolved node's `design` field, which nothing reads, is removed from the
  engine and the proto.
- The one-renderer tolerance is tightened so that a missing one-pixel divider
  or a missing small icon on the checkout design fails it, while today's
  measured GPU-against-PNG differences (scattered text-edge pixels, ≤ 0.2%)
  pass. The constraint states the rule as the test implements it.
- The developing page's test counts and module rows are true of the branch as
  it stands when this ticket merges, including ticket 06's tests if it merged
  first.

## Acceptance

- Tests: opacity on an instance, out-of-range opacity, and the tolerance
  failing on a removed divider and on a removed icon while passing on the
  committed GPU differences.
