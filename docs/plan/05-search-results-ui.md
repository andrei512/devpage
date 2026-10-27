# 05 — Grouped results and landing on the match

Read [the plan](README.md) first; it is the design. Ticket 04 changes the
endpoint to the plan's answer in parallel: build against that shape (a test
double serving it is fine until 04 merges).

## What

1. `DevpageSidebar`: results grouped by page as the plan's "The sidebar"
   says; keys across all rows; the clear button, Escape and the rest as now.
2. Landing on the match: opening a section result swaps the page in, scrolls
   to the first of its `words` inside that section, and marks every one of
   them in the section (`<mark class="devpage-found">`), until Escape or the
   next navigation; a `title_match` page opens at its top with nothing marked.
   Marking never touches code blocks, diagrams or widgets' own DOM.

## Docs (same commit as the code)

`docs/boxes/viewer/search/README.md` (the results, landing, marks); page-script
box if the swap path changes.

## Tests

Chrome tests (`#[ignore]`): results grouped by page with sections under them
and the "more" row; a section result lands scrolled to the first matched word
(not the heading) with the words marked; Escape clears the marks; a title
result opens at the top; existing sidebar and search tests stay green.

## Done when

Gates green, `devpage check docs` clean, committed on your lane branch.
