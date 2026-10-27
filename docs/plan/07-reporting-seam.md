---
id: "07"
blocked-by: []
boxes: [viewer/checking-links]
---

# Problems reported through a seam

The preview no longer prints. Each newly found problem goes to a reporter the
caller gives it: the terminal in `devpage`, a recorder in tests. The
prototype's leftovers land here too: the `<Notes>` tests move to the markup
module where the function now lives, and the preview's shared test helper
moves out of the production type.

## Acceptance

- No `eprintln!` in the preview or the link checker's core.
- What `devpage` prints for a new problem is unchanged.
- Gates green.
