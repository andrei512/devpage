---
id: "03"
blocked-by: ["01", "02"]
boxes: [viewer/server, viewer/comments]
---

# Comments in the store, carried across versions

## Build

- `threads` and `messages` tables, and a SQLite adapter of the comment
  `Store` port, scoped to a document. The page's existing comment mode works
  on `/d/<id>/` unchanged, through that adapter.
- The comments API in the
  [reference](../boxes/viewer/server/reference/README.md#api): list, start a
  thread on the latest version, reply, resolve; authors from
  `DEVPAGE_USER_HEADER`.
- **Re-anchoring on publish** as
  [reading a published document](../boxes/viewer/server/reading-a-published-document/README.md#comments)
  says: each open thread finds its quote in the new version by quote and
  context, records the version it was started on, and becomes detached when
  its text is gone; detached threads are listed at the top of the page until
  resolved. A write announces the page's comments on the change bus as
  locally.

## Acceptance

- The comment HTTP flow and re-anchoring tested with the SQLite adapter
  (kept, moved, detached, resolved), and the comments browser test run
  against a published document.
- Deleting a document deletes its threads.
