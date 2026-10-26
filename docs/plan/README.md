# Plan: fast pages, no flicker, browsers that download

devpage.ai, measured from ~120 ms away: every page 9–26 requests and
370–500 KB, ready after 1.1–1.6 s, a reload exactly as costly (every asset
`no-cache`, no validator), 5–6 round trips in a chain, a 5 MB design engine
re-downloaded each visit, and a blank frame on every slide change. Plus three
breakages found on the way. Seven investigations (one per issue) produced the
designs below; this plan reconciles them. Deleted once every lane is merged.

Story:

1. Quick fixes: a pinned Chrome, a working example image and install page,
   one tab title. ([01](01-quick-fixes.md))
2. Every devpage file content-addressed and cached for good; validators and
   304s for everything else. ([02](02-content-addressed.md))
3. A page names everything it needs in its head: one round trip.
   ([03](03-one-round-trip.md))
4. One swap path: built off-screen, unchanged widgets kept, slide changes
   never fetch. ([04](04-one-swap-path.md))
5. Fast slide PNGs. ([05](05-fast-slide-pngs.md))

Order: 01 and 02 start together; 03 after 02; 04 after 03; 05 after 01.

## The shared spine (02, 03 and 04 all depend on it)

- **One function makes every devpage-owned URL** (embedded assets, system
  widgets, project widget modules and their helper imports). The import map,
  the `modulepreload` links, `devpage.js`'s own references and the swap code's
  keep-key all use exactly what it returns. A preload whose URL differs by one
  byte from the import is a double download.
- **A widget's URL changes exactly when its compiled bytes would** (its source,
  its transitive helper imports, the known widget names, devpage's build).
  04 keys "is this mounted widget unchanged?" on that URL plus its server HTML.
- **Page HTML is revalidated, never cached blind**; its ETag lets the client
  skip a swap when nothing changed.

## Out of scope (decide later)

- Server-side diagram rendering (no pele, no React on docs pages) — needs a
  spike on label fit without a canvas.
- A release: the fontconfig fix reaches crates.io users only at the next
  publish, which the operator calls.
