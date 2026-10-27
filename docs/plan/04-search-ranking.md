# 04 — Search ranking and index

Read [the plan](README.md) first; it is the design.

## What

Everything in the plan under scope, one address, indexed by kind of text,
ranking, query, grouping, title intent, snippets, and the endpoint's answer —
in `src/search.rs` and `src/server.rs`. Start with the quality test.

## Order

1. Freeze a copy of devpage's docs into `tests/fixtures/search-docs/` and
   write `tests/search_quality.rs` (red): the ~40 queries and their expected
   results, assertions, MRR and success@3 printed.
2. Scope, one address, kinds of text, the spacing fix.
3. Ranking, query rules, grouping, title intent, snippets, `words`.
4. The new JSON answer (the plan's shape exactly — ticket 05 builds against
   it).
5. Tune until every query passes; set the floor at the result.

## Docs (same commit as the code)

`docs/boxes/viewer/search/README.md` (what's searched, ranking, the answer);
glossary; configuration reference (`search:` front matter); tests page.

## Done when

Gates green (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo test`, `cargo test -- --ignored` for tests you touched), `devpage check
docs` clean, committed on your lane branch. Report the query set's MRR and
success@3 before (today's ranking) and after, and the top 3 for `design`.
