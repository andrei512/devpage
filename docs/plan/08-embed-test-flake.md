---
id: "08"
blocked-by: []
boxes: [viewer/designs]
---

# The embedded-design Chrome test is deterministic

`an_embedded_design_draws_and_survives_a_page_edit` fails now and then in the
full parallel `--ignored` run and passes alone. Find what it races — the page
refetch, the stream, the paint — and make it wait for the state it checks; if
the race is in the page script rather than the test, fix the script.

## Acceptance

- The full `cargo test -p devpage -- --ignored` run is green ten times in a row,
  in parallel.
