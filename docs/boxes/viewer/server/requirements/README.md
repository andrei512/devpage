---
page-type: reference
---
# Requirements

What server mode must always do. Each requirement has its own page: the rule in
one sentence, why it exists, and how it is checked.

- [A published version never changes](a-published-version-never-changes.md) — a
  published version's files are kept byte for byte, and its link shows the
  same thing forever.
- [In server mode, a published document, version or comment must never be lost](nothing-published-is-lost.md)
  — losing the server's disk loses nothing.
- [One replica](one-replica.md) — server mode runs as one process;
  nothing coordinates two.
- [One server holds the envelope](one-server-holds-the-envelope.md)
  — on 2 CPUs and 4 GB, 10,000 documents and thousands of readers at their
  targets; the benchmark says whether it does.
- [Metrics name no document](metrics-name-no-document.md) — every
  label comes from a closed set, and metrics are served only on their own port.
