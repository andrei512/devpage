# 02 — Content-addressed files, validators everywhere

Read [the plan](README.md)'s shared spine first.

## Today (src/server.rs unless noted)

- Embedded assets (`asset`, ~:523–551): `include_str!` bodies, `no-cache`, no
  ETag, no compression locally (`pele.js` 709 KB raw). URLs hard-coded in
  `render.rs:217,233,239,333,340,484,489,541`, `designs.rs:513,518`,
  `devpage.js:176`, `design.js:16-17`. `assets/vendor/design-engine.wasm` is
  4,996,966 bytes.
- Widgets (`compiled`, ~:484–515): `no-cache`; version = the widget folder's
  latest mtime (`widgets.rs:125`), so any edit refetches every widget;
  `version_imports` (`widgets.rs:782`) stamps `?v=` on imports.
- Document files and pages (`file`, ~:198–322): no validators.
- Client: `load()` fetches `cache: "no-store"` (`devpage.js:217`); every SSE
  `change` re-downloads and re-swaps every open tab.

## Design

1. **Asset manifest**, built once (`LazyLock`): each embedded asset served at
   `/__devpage/<name>.<hash>.<ext>`, `Cache-Control: public, max-age=31536000, immutable`.
   Assets that reference others are hashed after rewriting, leaves first
   (wasm, engine.js → design.js → devpage.js). Bare names and stale hashes
   still answer, `no-cache` + ETag, never immutable — an old tab survives a
   redeploy. All emitters use the manifest (the shared URL function).
2. **Precompression**: `build.rs` writes br and gzip variants; the server
   negotiates from `Accept-Encoding` with `Vary: Accept-Encoding`. (Caddy's
   `encode` passes already-encoded responses through.) New build deps
   (brotli, flate2) get a dependencies-doc row.
3. **Widgets**: per-module `v=<hash>` over its source, the sorted set of its
   transitive relative imports' sources, the known widget names and the build
   id; each helper import carries the helper's own hash. Memoised by
   (path, mtime, size), invalidated by the watcher. A widget response is
   `immutable` only when the requested `v` equals the current hash, else
   `no-cache`.
4. **ETag + `no-cache`** everywhere else: one middleware sets a strong ETag
   (body hash) on 200 GETs and answers `If-None-Match` with 304. Expensive
   responses compute the ETag from inputs before rendering (design PNG: the
   resolved scene). Slide PNGs are 05's.
5. **Client**: `load()` uses `cache: "no-cache"`, keeps the current page's
   ETag, and skips swap and remount when unchanged; the build id joins
   `frameOf`, so a redeploy loads the page whole. (04 builds the swap; this
   ticket only adds the skip and the build-id check in today's `load()`.)

Check against devpage.ai behind Caddy that ETags survive `encode` (weak `W/`
is the fallback). Images keep their URLs (content-addressing image `src`
would break `markBroken`); they get ETags only.

## Tests

Every manifest URL immutable, bare/stale names `no-cache`; 304 on
`If-None-Match` for a page, an image, a design PNG; editing a helper changes
its importers' hashes only (replaces the `?v={version}` assertions at
`server.rs:1188-1195`, `widgets.rs:1001,1070`); br/gzip negotiation; Chrome
(`#[ignore]`): a reload sends only 304s/cache hits, and a change to an
unrelated file leaves the DOM untouched.

## Docs

serving-pages (asset table, the `no-cache` sentence), widgets/how-widgets-run,
page-script, live-updates (diagram, burst handling), rendering-a-page,
deploying (precompression, Caddy), engineering/dependencies.
