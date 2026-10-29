---
id: "02"
blocked-by: []
boxes: [viewer/server, viewer/checking-links]
---

# A page's problems, found once per version

In server mode, a page's problems are found on its first view and kept with
the version's router; every later view of that page shows the same bar
without checking ([a version's work is done once](../boxes/viewer/server/constraints/a-versions-work-is-done-once.md)).
A local `devpage` still checks on every view, since its files change.

## Acceptance

- A test views the same page of a version many times and the check runs once;
  `check` on `/metrics` counts one, not one per view.
- The kept problems are the same problems a fresh check finds: a page with a
  broken link and a broken `#heading` shows both on its first and its tenth
  view.
- Two versions of one document keep their own problems: a link broken in v1
  and fixed in v2 shows on v1's page only.
- A local `devpage` page still updates its bar when a link is fixed and saved.
