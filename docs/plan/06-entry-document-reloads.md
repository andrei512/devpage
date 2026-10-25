---
id: "06"
blocked-by: []
boxes: [viewer]
---

# The entry document reloads like any other

The design says a change to a document file takes effect on the next request,
with nothing to restart. Today the entry document's `root` (where `/` leads)
and a site's `watch` list are read once at start. Make the entry document
re-read when its file changes, so `/` and the watcher follow the file.

## Acceptance criteria

- A test changes the entry site's `root` and `/` follows it without a restart.
- A test changes its `watch` list and the watcher follows it.
- An entry document edited into an invalid state doesn't stop devpage: pages
  show the problem, and fixing the file recovers.
