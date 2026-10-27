# 01 — Search index and endpoint

Read [the plan](README.md) first; it is the design.

## What

1. `src/search.rs`: the tantivy in-RAM index of every in-sight document's
   sections, as the plan's "Search" says (fields, boosts, stored values,
   tokenizer, query: BM25 + prefix last word + fuzzy).
2. Built at start beside the document index; kept current by the watcher
   (`src/watch.rs` `Listeners::changed` — hook in where the index hears of a
   change). Out-of-sight files never indexed.
3. `GET /_devpage/search?q=…` (`src/server.rs`) returning the JSON in the
   plan. Empty or whitespace `q`: empty hits.

## Docs (same commit as the code)

- A new box `docs/boxes/viewer/search/README.md` (follow the other boxes'
  shape): what's indexed, ranking, how it stays current, the endpoint.
- Linked from the Viewer box's Topics; glossary (search index, hit);
  dependencies box: `tantivy`, with why; HTTP endpoints reference if there is
  one; code map.

## Tests

Red first: a section matched by a word in its body; title beats body; a typo
(`sidbar` → sidebar) still hits; a prefix while typing (`navig`) hits; a
slide's notes are searchable; an ignored file is not; editing a file updates
its hits after the watcher hears of it; deleting it removes them; the
endpoint returns the JSON shape with anchors and marked snippets.

## Done when

Gates green (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo test`, `cargo test -- --ignored` for tests you touched), `devpage check
docs` clean, committed on your lane branch. Report the index build time and
memory for devpage's own repo.
