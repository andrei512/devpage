---
id: "06"
blocked-by: ["05"]
boxes: [viewer/comments, viewer/comments/the-comment-store]
---

# The comment store port

Comments go through a comment store port — list, create, reply, resolve,
scoped to a document — with the YAML files as one adapter, unchanged on disk,
and an in-memory adapter for tests. Every write announces its own change on
the bus, so comment updates no longer depend on the watcher noticing a YAML
file; a thread file edited by hand still reaches open pages through the
watcher. Problems with unparseable thread files belong to the YAML adapter.

## Acceptance

- The comment HTTP flow is tested against the in-memory adapter.
- A write through the port reaches open pages without the watcher.
- The comment files and their format are byte-for-byte as today.
- Gates green including `cargo test -- --ignored`.
