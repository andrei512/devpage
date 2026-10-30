# Changelog

What changed in each released version of devpage, newest first. The three
crates — `devpage`, `devpage-core` and `devpage-design` — are released
together at the same version.

## 0.2.4 — 2026-10-30

- **Server metrics and a benchmark.** `devpage server` serves Prometheus
  metrics on their own port (`DEVPAGE_METRICS_PORT`). A version's problems
  are found once, the search index is built on the first search and capped at
  256 MB, and at most `DEVPAGE_MEMORY_CACHE_VERSIONS` versions are held in
  memory.
- **Faster saves on big sites.** Search keeps what is published between
  saves and loads each document once a pass: on a 5,000-page site a save
  re-indexes in about 19 ms, down from 450 ms.
- **Edits cost what they change.** An edit in a design's frame no longer
  grows with how many siblings it has, whatever the frame's layout.
- **A broken design keeps its last good drawing** until it can be drawn
  again.
- **Slide PNGs show neither comments nor problems.** A deck redraws its slide
  PNGs when an image or font its designs use is added or removed.
- **Links and search see widgets.** `devpage check` and search read a page as
  it is shown, so a link to a widget's heading lands and search finds it.
- **A path outside the served folder is refused.** A link leading out of it is
  reported as broken and never shows the file's absolute path.
- **Comments:** a comment on a missing page is refused, and so is a blank
  author.
- **Fix:** on the GPU, a design's images no longer vanish from tiles redrawn
  after an edit.
- **Internals:** the target-architecture migration — deeper modules with
  clearer names (`design`, `png`, `compositor`, `watch`, `published`, …) —
  and a browser test suite that runs in a fraction of the time.

## 0.2.3 — 2026-10-28

- **Server mode.** `devpage server` hosts published documents: `devpage
  publish` uploads one and gives back a link, and every version is kept, with
  a version menu and a banner on old versions. Comments live in SQLite and
  carry across versions. The CLI has kubectl-style contexts, `list`,
  `versions`, `comments`, `download` and `delete`, with `--json`. Deployment
  files are in `deploy/server/` and `deploy/bundled/`.
- **Designs have no size limit.** The 16,384-pixel cap is gone. The canvas
  paints only what is on screen, in tiles, and stays sharp at any zoom, on the
  GPU and without one. An edit repaints only the tiles it touches, in the
  browser and in the server, and a design's PNG is written band by band in
  bounded memory.
- **Keyed nodes.** A node keeps its identity across inserts and reorders, so
  an edit — inserting at the top of a list among them — updates only what it
  changed. A save resolves again only what it reached.
- **The sidebar lists each page once**, at its highest listing; a page listed
  again lower down, or a link back to a parent, is left out of the sidebar.
- **The front page** drops the open-by-link box.
- **Internals:** a pure `devpage-core` crate with ports only where adapters
  vary, and faster gates — CI now runs fmt, clippy and the tests on every pull
  request.

## 0.2.2 — 2026-10-27

- **Fonts in designs:** `<Text font="…">`, with names mapped to files in
  `themes.yml`'s `fonts:`; a design can use several.
- **Fix:** a design in a tab other than the first now draws when the tab
  opens.

## 0.2.1 — 2026-10-27

- **Themed designs:** `themes.yml` gives a design light and dark colours; an
  embed follows the page's theme and has its own switch.
- **Diagrams drawn by the server**, and a broken Mermaid diagram is reported
  by `devpage check` and the page's problems bar.
- **The sidebar and search:** a collapsible sidebar that remembers what was
  opened, and search across every document.
- **Faster pages:** content-addressed files, one round trip, one swap path,
  fast slide PNGs and a lean watcher that watches only what is in sight.
- `--no-comments` for a read-only public site.
- The narrator takes an audience, goal, tone, pronunciations and a delivery
  per slide.

## 0.2.0 — 2026-10-26

- **Designs:** UI drawn as code, on a GPU canvas and as a PNG.
- **Widgets:** React components in Markdown.
- **`devpage video`:** a deck narrated and recorded to mp4, with speaker
  notes and slide PNGs.
- The navbar, a logo, the light/dark switch and the comment mode switch.
- A slow external link is no longer reported broken on its first try.

## 0.1.0 — 2026-10-25

The first release: sites, docs and slides from Markdown; comments; link
checking; PDF; deploying.
