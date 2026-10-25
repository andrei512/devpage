---
id: "13"
blocked-by: []
boxes: [viewer/comments]
---

# The selected-text comment Chrome test is deterministic

`comments::browser::selected_text_becomes_a_thread_in_the_margin_and_in_the_store`
still fails about one full parallel `--ignored` run in four. Find what it
races and make it wait for the state it checks; if the race is in the page
script, fix the script.

## Acceptance

- `cargo test -p devpage -- --ignored`, in parallel, green ten times in a row.
