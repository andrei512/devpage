# The vendored icons

What icons devpage can draw — the Lucide set, plus GitHub's mark — where they
came from, their versions and licences, how devpage draws them, and how to
upgrade them. It is a mechanism page under the [viewer](../README.md), for
someone about to replace the set; using icons in a site's navbar is in
[Organizing navigation](../organizing-navigation/README.md#the-navbar).

## What is in the repository

| File | What it is |
| --- | --- |
| `core/assets/vendor/lucide.json` | Every [Lucide](https://lucide.dev) icon — 1,877 of them — from `lucide-static` **1.55.0** (published 2026-10-10): its `icon-nodes.json`, minified. Each icon is a list of SVG elements on a 24 × 24 grid, drawn with strokes. |
| `core/assets/vendor/lucide.LICENSE` | Lucide's licence: ISC, with the MIT notice for the icons derived from Feather. |
| `core/src/icons.rs` | GitHub's mark, one filled path, from [Simple Icons](https://simpleicons.org) **16.34.0** under CC0 1.0 — Lucide carries no brand marks. |

## How devpage uses them

`core/src/icons.rs` compiles `lucide.json` into the binary with `include_str!` and
reads it once, on first use. An icon is drawn on the server, as inline SVG in
the page — `<svg class="icon">`, in the text color (`currentColor`) — so a
page carries only the icons it shows and they follow the theme. A name the set
doesn't have is a problem ([Checking links](../checking-links/README.md)).
devpage draws icons in a site's navbar and in the theme selector (`moon` and
`sun`, and `menu` and `chevron-down` for the navbar's own controls).

## Upgrading them

<Steps>

1. Fetch the icons and the licence
   Pick a `lucide-static` version published at least two weeks ago.

   ```sh
   curl -so nodes.json https://cdn.jsdelivr.net/npm/lucide-static@<version>/icon-nodes.json
   curl -so core/assets/vendor/lucide.LICENSE https://cdn.jsdelivr.net/npm/lucide-static@<version>/LICENSE
   python3 -c 'import json; json.dump(json.load(open("nodes.json")), open("core/assets/vendor/lucide.json", "w"), separators=(",", ":"))'
   ```

2. Check every icon devpage names still exists
   Lucide renames icons now and then; the icons test names devpage's own, and
   the docs check covers the docs' navbars.

   $ cargo test
   $ cargo run -- check docs

3. Update this page
   The version, count and date.

</Steps>

GitHub's mark changes rarely; to replace it, copy the `d` of the path in
Simple Icons' `icons/github.svg` into `BRANDS` in `core/src/icons.rs`.
