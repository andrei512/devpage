---
page-type: landing
---
# Write

How each kind of document is written and how a site is laid out: the four
kinds and their document file, a site with its sidebar and its code reference,
a doc and a deck. It is a part of the [viewer](../README.md); what a page
itself can hold is on [On the page](../on-the-page/README.md).

```mermaid
flowchart LR
  top["Write"] --> n0["Writing a document"]
  top["Write"] --> n1["Making a site"]
  top["Write"] --> n2["Organizing navigation"]
  top["Write"] --> n3["Code reference"]
  top["Write"] --> n4["Writing a doc"]
  top["Write"] --> n5["Making slides"]
  click n0 href "./writing-a-document/README.md" "Writing a document"
  click n1 href "./making-a-site/README.md" "Making a site"
  click n2 href "./organizing-navigation/README.md" "Organizing navigation"
  click n3 href "./code-reference/README.md" "Code reference"
  click n4 href "./writing-a-doc/README.md" "Writing a doc"
  click n5 href "./making-slides/README.md" "Making slides"
```

## Topics

- [Writing a document](writing-a-document/README.md) — the four kinds, the
  document file, the served folder, links between pages and documents, and the
  Markdown that works.
- [Making a site](making-a-site/README.md) — a site's document file, root page,
  sidebar, navbar, logo, theme switch and `404.md`.
- [Organizing navigation](organizing-navigation/README.md) — how a site's Topics
  sections build the sidebar, and how to lay out its reading order.
- [Code reference](code-reference/README.md) — a page for every module, type and
  function of a site's code, read with tree-sitter, live, and checked when
  prose links to it.
- [Writing a doc](writing-a-doc/README.md) — chapters, the outline, and the
  PDF's paper, header and footer.
- [Making slides](making-slides/README.md) — one file per slide, the stage, the
  keys, and printing a deck.
