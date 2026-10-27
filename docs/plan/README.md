# Plan: watch only what's in sight

## Why

devpage watches the whole served folder: one FSEvents stream, latency 0,
per-file events (`notify` 8.2, `src/fsevent.rs:300`). fseventsd delivers every
write under `target/`, `node_modules/` and `.git/` to devpage, which then throws
it away in `sift`. Measured on an M-series Mac, 400k file writes under
`target/` of the devpage repo:

| devpage processes | fseventsd CPU |
|---|---|
| none | 5.6 s |
| 1 | 9.6 s |
| 8 | 42.8 s |

Each devpage adds ~4.5 s of fseventsd CPU per 400k ignored writes. Sifting made
devpage's own side cheap; the cost moved to fseventsd. The fix is to never ask
for those events: **nothing out of sight is watched.**

## The design

**The watch set.** From the index's rules (`Index::ignored`: `.git`,
`.devpage`, `.gitignore`), the served folder splits into:

- **deep folders** — the largest folders with nothing out of sight anywhere
  under them. Watched recursively.
- **shallow folders** — folders in sight that hold something out of sight
  (the served folder itself, usually). Watched non-recursively: their entries
  and the files directly in them.

Out-of-sight folders are not watched at all, with two exceptions that already
get past sifting today and must keep working:

- `.devpage/comments` (comment threads) is in sight; `.devpage` itself is
  shallow.
- An ignored folder a site's `watch` list can name something in. A pattern's
  literal prefix (its components before the first one with a glob character),
  joined to the site's folder, decides: the ignored folder is watched when it
  holds that prefix or the prefix holds it. So `watch: ["build/*.svg"]` keeps
  `docs/build/` watched; nothing names `target/`.

**Keeping it current.** The set is recomputed (and the watches diffed: only
what changed is added or removed) when a burst holds a folder created or
removed in sight, a `.gitignore`, or a site document file (its `watch` list).
A folder that turns out to hold something newly out of sight (`cargo build`
in a nested crate creates `crate/target/`) makes its deep folder shallow; its
events until then are dropped by sifting as now.

**Platforms.**

- **macOS:** devpage's own watcher, not `notify`'s. Deep folders: FSEvents
  (`fsevent-sys`, already in the tree via `notify`), one stream per deep
  folder so adding or removing one restarts nothing else, file events,
  latency `BURST` (fseventsd coalesces; no `NoDefer`). Shallow folders: kqueue
  (`EVFILT_VNODE`) on the folder (entries added, removed, renamed) and on each
  file directly in it (written, deleted, renamed). The callbacks feed the same
  `intake` handler as today, as `notify::Event`s, so sifting, bursts and
  listeners don't change.
- **Linux:** `notify`'s inotify watcher, `NonRecursive` on every in-sight
  folder (inotify is per folder anyway), so ignored folders get no inotify
  watch.
- **Elsewhere:** `notify`'s recommended watcher, recursive on the served
  folder, as now — say so in the docs.

Sifting stays: it is the backstop for the gap between a folder going out of
sight and the re-plan, and for platforms that can't leave a subtree out.

## Done when

The measurement above, re-run on the branch: with 1 and with 8 devpages
serving the devpage repo, fseventsd CPU for 400k writes under `target/` is
within noise of none. Recorded in the PR.

## Tickets

- [01 — Watch only what's in sight](01-watch-in-sight.md)
