# Plan: a sidebar that scales, and search

## Why

The sidebar (`#devpage-nav`, built by `sidebar` in `src/render.rs` from
`nav::tree`) shows every level of the tree, always expanded, as static HTML
with hand-written JS around it. On devpage's own docs it is already a long
list, and there is no way to find a page but scrolling. React is now part of
every page, so the sidebar becomes a React component that holds its own state,
and search becomes a real full-text engine on the server.

## The design

### `DevpageSidebar`

A system widget devpage owns. The `Devpage` prefix marks devpage's own chrome,
so it never clashes with a project's widgets: a project may have its own
`Sidebar`. It is not replaceable by a project widget.

- **First paint.** The server still renders the sidebar's HTML (collapsed as
  below, the current branch open), and embeds the tree as JSON in the page.
  React takes the sidebar over from that markup, producing the same DOM, so
  nothing moves or flashes when it does.
- **Collapsed by default.** The top level shows; the branch down to the
  current page is open; every entry with children has a chevron that opens
  or closes it. What the reader opened is remembered per document
  (`localStorage`, wrapped in try/catch; works without it).
- **Survives navigation.** The page swap keeps the sidebar mounted: its open
  state and the search box's text persist across page changes; only the
  current-page highlight moves, and its branch opens.
- **Group headings** (`###` in Topics) render where they are in the tree, as
  today.
- Sites with `sidebar: false` get none, as today.

### Search

- **Engine:** `tantivy` (0.27), index in RAM (`Index::create_in_ram`), BM25.
- **What is indexed:** every in-sight document in the served folder — site
  pages, docs, slides (their slide text and `<Notes>`) — one entry per
  section (a page split at its headings). Fields: page title, heading, body
  (boosted in that order), plus stored: URL with the heading's anchor, the
  document's title and path, the page's place in its document's nav.
- **Kept current:** built at start, updated by the watcher: a changed file's
  entries are deleted and added again; a removed file's deleted. A
  `.gitignore` change re-indexes what it governs. Out-of-sight files are never
  indexed.
- **Query:** words matched with BM25; the last word as a prefix (so results
  come while typing); fuzzy (Levenshtein 1 for words of 4+ letters, 2 for
  8+). Tokenizer: tantivy's default with lowercasing and English stemming.
- **`GET /_devpage/search?q=…`** returns JSON: hits (title, heading, URL with
  anchor, snippet with the matched words marked, document title and path,
  nav trail), best first, at most 50. Served whatever `--no-comments` says;
  never serves anything outside the served folder or out of sight.
- **UI in `DevpageSidebar`:** a search box at the top of the sidebar; `/` or
  ⌘K / Ctrl+K focuses it. While it holds text, results replace the tree,
  grouped by document, the current document's group first; arrow keys move,
  Enter opens (through the normal page swap, landing on the heading), Escape
  clears and brings the tree back. Requests are debounced and a stale
  response never overwrites a newer one.

## Tickets

- [01 — Search index and endpoint](01-search-index.md)
- [02 — `DevpageSidebar`](02-devpage-sidebar.md)
- [03 — Search in the sidebar](03-search-ui.md) — after 01 and 02

01 and 02 run in parallel.
