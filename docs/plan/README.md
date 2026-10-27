# Plan: search that finds what a reader means

## Why

Searching `design` on devpage's own docs gave 50 hits from 8 pages (one page
held 12), a top snippet of Mermaid source glued to prose
("engine.flowchart LR"), clicks that land on a section heading rather than the
match, the same file under two addresses, and test fixtures. The design below
came out of a debate between a retrieval view and a reader view; both agreed
on all of it.

## The design

**Scope: what's published.** Only documents reachable from the served
document are searched: a site's nav tree (every page it lists), and the
chapters and slides of every document those reach (`Target::Document`), and
so on. Fixtures, plans and orphaned files are still served but never
searched. Front matter `search: false` leaves a reachable page out;
`search: true` puts an unreachable one in.

**One file, one address.** A file that is both a site page and a doc chapter
(or a slide) is indexed once, at the address of the document that owns it as a
chapter or slide.

**Indexed by kind of text.** Per section (a page split at its headings):

- `title`, `heading` — and the section's heading path (page title, chapter
  title, enclosing headings), its depth, its order on the page, and the page's
  position in the nav (fast fields);
- `prose` — paragraphs, lists, tables, a diagram's labels (never its source),
  a widget's text attributes (never its markup); blocks joined with spaces;
- `code` — fenced and inline code, tokenized for code (`snake_case`,
  `camelCase`, `::` split, no stemming), low boost;
- `notes` — slide `<Notes>`, low boost; `meta` — front matter description and
  keywords, high boost;
- `title_exact`, `heading_exact` — keyword-tokenized, for whole-title matches.

Plus one **page record** per page: title and lede (first paragraph).

**Ranking.** Text match first, structure only to break near-ties:

- a dis-max across the text fields (small tie-breaker), so one word doesn't
  score in three fields at once;
- optional phrase bonuses (slop 0 and 3) on heading and prose;
- a large boost when the query is a whole title or heading;
- then `tweak_score`: a little for shallower sections, earlier on the page,
  earlier in the nav.

**Query.** Every word must match (stopwords excepted); if none do, any word,
with a coverage penalty. `"quoted"` is a phrase, `-word` excludes. Fuzzy
(1 edit at 4+ letters, 2 at 8+) only for a word not in the index. The last
word also as a prefix, including against `heading_exact`.

**Grouped by page, on the server.** Top 200 sections, grouped by page; a page
scores as its best section; at most 10 pages, each with at most 3 sections and
a count of the rest.

**Title intent.** When the query matches a page's title, that page comes first
as a page result, with its lede, and opens at its top.

**Snippets and landing.** A snippet is cut from prose only: the window where
the query's words cluster densest, at sentence bounds; a section matched only
by its heading shows its first sentence. Each section result carries the
words that matched as they appear in its text. Opening it swaps the page in
as now, scrolls to the first of those words in that section (not the
heading), and marks them in the section until Escape or the next navigation.

**Measured.** `tests/search_quality.rs`: ~40 queries over a frozen copy of
devpage's docs (`tests/fixtures/search-docs/`, excluded from devpage's own
search by scope), each naming the URL expected first or in the top 3 —
`design` → the Designs page, `outbox`, `frame ids`, `gitignore watcher`, a
typo, a code identifier, a phrase. It asserts each, and reports MRR and
success@3 against a floor. Written before the ranking is tuned.

## The endpoint's answer

```json
{
  "pages": [
    {
      "title": "Designs",
      "url": "/docs/boxes/viewer/designs/README.md",
      "trail": ["System catalog", "Viewer", "Designs"],
      "document": { "title": "devpage", "path": "/docs/devpage.yml" },
      "kind": "page",
      "title_match": true,
      "lede": "A UI an agent writes as code…",
      "sections": [
        {
          "heading": "A first design",
          "path": ["Designs", "A first design"],
          "url": "/docs/boxes/viewer/designs/README.md#a-first-design",
          "snippet": "…the <mark>design</mark> file…",
          "words": ["design", "designs"]
        }
      ],
      "more": 7
    }
  ]
}
```

`kind`: `page`, `chapter` or `slide`. `title_match` pages open at `url`
(their top).

## The sidebar

Results stay in the sidebar, in place of the tree, as now. They're grouped by
page: the page row (trail, title, lede when `title_match`) and its sections
under it (heading, snippet), then "N more in this page", which opens the
page. ↑↓ move through every row, Enter opens. Where results show — the
sidebar or a ⌘K pop-up — is an open question for the operator; don't build
a pop-up.

## Tickets

- [04 — Search ranking and index](04-search-ranking.md)
- [05 — Grouped results and landing on the match](05-search-results-ui.md)

04 and 05 run in parallel against the answer above.
