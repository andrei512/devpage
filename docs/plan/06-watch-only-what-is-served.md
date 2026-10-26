# 06 — Watch only what is served

`src/watch.rs` watches the whole served folder recursively
(`watcher.watch(&root, RecursiveMode::Recursive)`) and filters afterwards:
for every path in every event it `canonicalize()`s (a syscall) before deciding
it doesn't care. Served from a repository root, that subscribes devpage to
`target/` (11 GB here, rewritten by every cargo build), `.git/`,
`node_modules/` and every other generated tree; a build storm becomes
millions of callbacks, each doing syscalls, and a slow FSEvents client keeps
fseventsd's queues full. (Measured: fseventsd's CPU is mostly the machine's
build churn — it logs every change on the volume whoever watches — but devpage
should not add a slow consumer of events it throws away.)

## Design

- **Decide cheaply, before any syscall, whether a path can matter**: drop it
  when any component is `.git` or a folder the served folder's `.gitignore`
  files ignore (the `ignore` crate's matcher, built once, rebuilt when a
  `.gitignore` changes), except `.devpage/` (comment threads) and the files
  `watch` names. Canonicalize only paths that survive.
- **Batch**: collect a burst of events (e.g. 50 ms) and handle each path once
  per batch; one `change` per batch, not per file.
- **macOS**: notify's FSEvents backend — check whether its stream latency can
  be raised (coalescing in fseventsd itself) and whether ignored subtrees can
  be left out of the stream (e.g. watch the non-ignored top-level entries
  instead of the root, adding new ones as they appear). Measure, keep what
  helps.
- **Tests own their watchers**: every test that starts one drops it (check the
  watch and server tests); no test watches a folder bigger than its fixture.

## Measure

Before and after, on this machine: devpage serving `/Users/andrei/projects/devpage`
during a `cargo build` in that checkout — devpage's CPU and its callback count
(log a counter), and fseventsd's CPU. Report the numbers.

## Tests

Events under `target/`, `.git/` and a `.gitignore`d folder never reach the
listeners and never call `canonicalize`; `.devpage/comments` and `watch`-named
files still do; a burst of N changes to one file sends one `change`; a new
`.gitignore` rule takes effect without a restart. The existing watch tests
keep passing (they need working file events — fseventsd may be stuck on this
machine; report, don't chase).

## Docs

live-updates (what is watched and what is ignored, batching), serving-pages if
it describes the watcher, the tests doc.
