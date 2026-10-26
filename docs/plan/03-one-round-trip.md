# 03 — One round trip

Read [the plan](README.md)'s shared spine first. Builds on 02's URL function.

## Today

HTML → `style.css` + `pele.js` (709 KB raw, classic `<script>` at the end of
`<body>`, `render.rs:239,340,488,547`; served on every page) + `devpage.js` →
`mountWidgets` awaits `import("react")` before any widget (`devpage.js:73`);
`drawDesigns` starts `design.js` (`:176`) → widget modules + engine.js →
4.9 MB wasm → helper imports (`frames?v=…`, `widgets.rs:781`) and
`__devpageUses` waves (`devpage.js:48`) → images. 5–6 sequential round trips.

## Design

- The server computes each page's **load set**: every `data-widget` name in
  the rendered content; `Diagram` if a `<pre lang="mermaid">`; each module's
  `__devpageUses` and versioned relative imports (from `compile_cached`); React
  if any widget; `design.js` + engine + wasm if a `canvas.design`. Transitive
  closure.
- Emit it in `<head>` after the import map (`Widgets::head`):
  `<link rel="modulepreload">` per module with the exact URL the import uses;
  `<link rel="preload" as="fetch" crossorigin fetchpriority="low">` for the
  wasm.
- **pele becomes the Diagram widget's import**: served as a module (append
  `export default pele;`), imported by `Diagram.tsx`; the `<script src=pele.js>`
  tags go. Fetched only with a diagram, preloaded when there is one.
- `devpage.js` gets `defer` in `<head>` (stays classic; it reads
  `document.currentScript`).
- `mountWidgets` loads widget modules in parallel with React.
- `load()` copies a fetched page's preload links into the head before
  mounting (04 then owns `load()`; keep this a small helper it can call).

Expected: cold deck ~3 round trips (~0.8 s from 120 ms away, was 1.6 s), no
pele on the deck; warm 1 round trip with 02.

## Tests

`render.rs` units: a diagram page preloads Diagram and pele; a plain page has
neither pele nor React; a deck whose widget imports a helper preloads
`frames?v=<hash>`; a design page preloads the design chain; update the
script-tag assertion (~`render.rs:1037`). Chrome `#[ignore]`: on a cold load
no module request is started by script (all parser or preload), and a plain
page never fetches pele or react.

## Docs

vendored-pele ("Every page loads it" stops being true), serving-pages,
page-script, rendering-a-page, widgets/system-widgets (Diagram), diagrams,
design-engine, vendored-react.
