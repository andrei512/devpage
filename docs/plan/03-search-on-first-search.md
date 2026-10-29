---
id: "03"
blocked-by: []
boxes: [viewer/search, viewer/server]
---

# A version's search index, built on its first search

In server mode, a version's router is made without a search index. The
version's first search builds it, commits it once and keeps it with no writer
and no indexing threads. The most recently searched versions' indexes are
kept, up to 256 MB in all; past that the least recently searched is dropped
and built again on its next search
([what a version holds in memory](../boxes/viewer/server/storage/README.md#what-a-version-holds-in-memory)).
A local `devpage` keeps its live index as now.

## Acceptance

- Viewing hundreds of versions without searching them adds no threads and no
  search memory; the server's thread count stays flat.
- A version's first search answers as a local search of the same files does;
  its second builds nothing.
- Past 256 MB of indexes, the least recently searched is dropped, and searching
  it again still answers.
- A local `devpage`'s search still follows edits.
