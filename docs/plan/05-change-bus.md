---
id: "05"
blocked-by: ["01"]
boxes: [viewer/live-updates]
---

# A typed change bus

Live updates travel as typed changes — a refresh, or the comments of a page —
each scoped to the document it concerns, instead of strings on one global
channel. The file watcher is one producer. The cache invalidation it does today
(designs, widgets, the index, the entry document) becomes the watcher's own
listener, not traffic on the bus. The slide-PNG cache and the event stream
subscribe to the bus. No module keeps a global channel.

## Acceptance

- A page's event stream carries only its document's changes (one document
  today, so the page sees what it sees now).
- The watcher's invalidation is tested without the bus; the bus is tested
  without notify timing.
- Gates green including `cargo test -- --ignored`.
