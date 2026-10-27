# 03 — Search in the sidebar

Read [the plan](README.md) first; it is the design. Builds on 01 (the
endpoint) and 02 (`DevpageSidebar`).

## What

The search box at the top of `DevpageSidebar` and its results, as the plan's
"UI in `DevpageSidebar`" says: `/` and ⌘K / Ctrl+K focus; results replace the
tree while there is text, grouped by document, the current document first;
snippets with matches marked; arrows, Enter (normal page swap, landing on the
heading), Escape; debounced; a stale response never overwrites a newer one;
text persists across swaps.

## Docs (same commit as the code)

The search box (`docs/boxes/viewer/search/`): the UI and keys. Feature
catalog (`docs/features/`): an entry for search. Getting-started mention if
it lists the reader's tools.

## Tests

Chrome tests (`#[ignore]`): typing shows grouped results; Enter lands on the
heading; Escape restores the tree; `/` focuses; a slow earlier response
doesn't replace a later one.

## Done when

Gates green, `devpage check docs` clean, committed on your lane branch.
