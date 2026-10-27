# 01 — Watch only what's in sight

Read [the plan](README.md) first; it is the design. One ticket.

## What

1. **The watch set** (platform-free, unit-tested): from the served folder and
   the index, the deep folders and the shallow folders, with the two
   exceptions in the plan (`.devpage/comments`; ignored folders a `watch`
   pattern's literal prefix overlaps). Computed by walking only what's in
   sight — never descend into an ignored folder except an exception.
2. **macOS watcher:** FSEvents stream per deep folder (file events, latency
   `BURST`, no `NoDefer`), kqueue for shallow folders and the files directly
   in them, feeding `intake` as `notify::Event`s. Stopping on drop, as now.
3. **Linux:** `notify` inotify, `NonRecursive` per in-sight folder.
4. **Re-plan** on a burst holding a folder created/removed in sight, a
   `.gitignore`, or a site document file; diff and apply.
5. Keep sifting unchanged.

## Docs (same commit as the code)

- `docs/boxes/viewer/live-updates/README.md`: replace the "On macOS, `notify`
  watches through one FSEvents stream…" paragraph and update "The watcher"
  section: the watch set, deep/shallow, re-plan, per platform, and why
  (fseventsd's cost, with the measurement).
- `docs/boxes/engineering/dependencies/README.md`: `fsevent-sys` and whatever
  kqueue binding you use (prefer `libc`, already a dependency, if it is) as
  direct macOS dependencies, with why.
- Glossary if it has watcher terms: in sight / out of sight, deep / shallow
  folder.
- Code map if it lists `src/watch.rs`'s parts.

## Tests

Red first, on real folders (temp dirs), counting the raw events the platform
watcher hands `intake` (a test counter, like `CANONICALIZED`):

- writes under an ignored `target/` and `.git/` produce **zero** raw events;
- a write to a file directly in the served folder, and in a deep folder,
  each arrive;
- a new file directly in the served folder arrives; a new folder in sight,
  then a file in it, arrives;
- adding `gen/` to `.gitignore` stops `gen/`'s events after the re-plan;
- a nested `crate/target/` created after start stops producing events after
  the re-plan;
- `.devpage/comments/…` writes arrive; other `.devpage/` writes don't;
- a `watch`-named ignored folder (`build/*.svg`) is watched;
- the watch set itself: unit tests on deep/shallow for a few layouts.

## Measurement

Re-run the plan's fseventsd measurement (400k writes under `target/` of this
repo, fseventsd CPU from `ps -o time= -p $(pgrep -x fseventsd)`, with 0, 1 and
8 `devpage docs --port 307x` running from the repo root; ports 3030/3031 are
off limits). Put the table in your final report.

## Done when

Gates green (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo test`, `cargo test -- --ignored`), `devpage check docs` clean, the
measurement shows devpage adds no fseventsd CPU within noise, committed on
your lane branch.
