---
id: "02"
blocked-by: []
boxes: [viewer, viewer/writing-a-document, viewer/checking-links]
---

# One served-folder value

The served-folder boundary — resolve a link or path inside the folder,
following symbolic links, refusing anything outside it — has one owner: a
served-folder value that resolves, reads, lists and turns a file into its
address. The hand-written copies of join, canonicalize, starts-with and
is-file in document loading, the navbar, the navigation tree, the link
checker and the design store go. It is a value, not a trait: the disk is its
only adapter.

## Acceptance

- No module outside the served-folder value canonicalizes or prefix-checks a
  document path.
- The boundary tests (`/../file`, a symbolic link out) run against the value
  directly, and still pass through the server.
- Pages, links and problems unchanged; gates green.
