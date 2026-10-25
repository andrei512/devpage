---
id: "02"
blocked-by: []
boxes: [viewer]
---

# Link checking, production-quality

- External links in the preview: checked in the background, once per address
  for the life of the process, deduplicated; a failure reaches open pages that
  link to it through a live update, and prints once on the terminal.
- `devpage check` deduplicates external requests by address.
- Problems print once each on the terminal in the preview, as
  `file:line: problem`.
- `cargo test` checks devpage's own docs (internal links and anchors only) and
  fails on any problem; fix every problem it finds in the docs.

## Acceptance criteria

- A test with a local HTTP server returning 404 shows the external problem on
  the page after the background check, and `devpage check` exits 1 on it.
- The docs-check test passes on this branch.
- The checking-links page describes the code as built.
