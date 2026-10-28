# Changelog

What changed in each released version of devpage, newest first. The three
crates — `devpage`, `devpage-core` and `devpage-design` — are released
together at the same version.

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
