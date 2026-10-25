# Navigation tree

How `src/nav.rs` reads the sidebar tree out of the pages' `## Topics`
sections — finding the section, taking each item's first link, resolving it to
a file, grouping by `###` headings and stopping at pages already seen — and how
`src/render.rs` turns the tree into the sidebar's HTML with the current page
highlighted. It is a mechanism page under the [viewer](../README.md), for
someone about to change either file; how to use Topics is in
[Organizing navigation](../organizing-navigation/README.md).

## The data

```mermaid
flowchart LR
  tree["nav::tree(document)"] --> groups["list of Group"]
  groups --> group["Group<br/>title, or none"]
  group --> entry["Entry<br/>label, file"]
  entry -->|"children"| groups
  click tree href "#building-the-tree-and-stopping-cycles" "Building the tree"
  click group href "#reading-a-topics-section" "Groups come from ### headings"
  click entry href "#resolving-a-link" "Resolving a link"
```

- A **`Topic`** is one link found in a Topics section, before resolving: its
  group (the `###` heading above it, if any), its label and its `href`.
- An **`Entry`** is a resolved page in the tree: its label, its canonical file
  path and its own children.
- A **`Group`** is a run of entries under one `###` heading; `title` is `None`
  for entries listed before any group heading.

`nav::tree` returns the root page's children as groups; the root page itself is
not an entry — the sidebar's document-title link stands for it.

## Reading a Topics section

`nav::topics(source)` parses the page with comrak (default options) and walks
the **top-level** blocks of the document in order:

- A heading of level 1 or 2 ends any Topics section and resets the group; it
  starts one when it is level 2 and its text, trimmed, is exactly `Topics`.
- Inside Topics, a level-3 heading sets the current group to its trimmed
  text. Deeper headings are ignored.
- Inside Topics, each top-level list contributes its items in order. For each
  item, the **first link** found anywhere inside it — including in a nested
  list — gives the label (the link's text, with inline code included) and the
  `href`. Items without a link are skipped.

Lists that aren't top-level blocks — inside a block quote, say — are not read.
A page may have more than one `## Topics` heading; each is read.

## Resolving a link

`resolve` turns an `href` into a file, or drops the topic:

1. An `href` containing `://` or starting with `#` is dropped.
2. Any `#fragment` is cut off.
3. The rest is joined to the **listing page's folder** and canonicalized;
   failure — usually a missing file — drops it.
4. It is kept only if it lies inside the document's folder, ends in `.md` and
   is a file.

Dropped topics produce no warning. The `href` is used as written, without
percent-decoding.

## Building the tree and stopping cycles

`tree` starts with a set `seen` holding the root page, then calls `children`
on the root. `children` reads the page from disk (an unreadable page has no
children), and for each resolved topic:

- inserts the file into `seen`; if it was **not** there before, the entry's
  children are built recursively, otherwise the entry gets none;
- appends the entry to the last group if that group has the same title, or
  starts a new group.

`seen` is shared by the whole walk, which is depth-first in list order. So a
page appears wherever it is listed, but only its first appearance in that order
is expanded, and a link back up to an ancestor — or to the root — ends there.
This is the case the test `builds_the_tree_and_stops_at_cycles` covers.

Grouping compares with the *last* group only, so the same `###` title appearing
twice, with another group between, gives two separate groups.

The tree is rebuilt for every page request, reading every page it reaches; no
result is kept between requests.

## Rendering the sidebar

`render::navigation` renders the groups as nested `<ul>` lists:

- a group with a title becomes `<li class="group"><span>title</span></li>`
  before its entries;
- each entry becomes `<li><a href="/path">label</a>`, followed by its
  children's list, if any;
- labels and group titles are HTML-escaped.

The `href` is `/` plus the file's path relative to the document's folder.
**Current-page highlighting** compares that with the requested page's own path
built the same way: every matching entry gets `class="current"` — more than one
when a page is listed twice. The document-title link above the tree (class
`document-title`) gets `current` when the page is the root page.

The sidebar is part of every page's HTML, inside `<nav id="nav">`, so a
[live update](../live-updates/README.md) or an in-place link swaps it together
with the content.
