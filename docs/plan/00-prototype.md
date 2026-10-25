# Prototype report: designs

Measured: started 20:42:16, stopped 21:02 — about 20 minutes of the 30, with
the vello amendment arriving at minute 10.

## What runs, end to end

- `examples/checkout/{button,checkout,confirmation}.design`: imports, props,
  auto layout, fills, strokes, radius, shadow, an ellipse, `onClick`.
- `GET /examples/checkout/checkout.design.png` → parse → resolve → taffy →
  parley → display list → `vello_cpu` → PNG (≈110 ms in a debug build).
- `GET /…/checkout.design` → canvas page → module from `wasm-bindgen` glue →
  `POST /devpage.design.v1.Designs/Watch` (tonic-web, same listener as axum) →
  scene, then patches → `vello` on WebGPU in Chrome (`data-backend="gpu"`),
  and the `vello_cpu` fallback when WebGPU is off (`--disable-gpu`). Both
  checked by headless-Chrome screenshots.
- A save that changes one text sends a ≈107-byte patch of that one node; the
  first scene is ≈1 KB.
- Clicking `Pay $42` navigates (hit-testing in the module; `back` uses history).

## What I touched

- New workspace: `design/` (`devpage-design`: `parse`, `resolve`, `render`,
  `wire`, `proto`), `design/wasm/` (cdylib: raw exports for the scene, `apply`,
  `click`, CPU `paint_scene`; `gpu.rs` with wasm-bindgen + vello/wgpu).
- `build.rs` (devpage): protox + tonic-prost-build for the service; nested
  `cargo build --target wasm32-unknown-unknown` into `OUT_DIR`; then
  `wasm_bindgen_cli_support::Bindgen` as a library. `design/build.rs`: protox +
  prost-build for the messages, shared by server and module.
- `proto/design.proto`, `rust-toolchain.toml`, Inter faces + OFL licence in
  `design/fonts/`.
- Server: `src/designs.rs` (served-folder `Files`, shared renderer, Watch
  service, canvas page), routes in `src/server.rs`, `assets/design.js`.

## Where the difficulty actually was

- Not where expected. Nested cargo in `build.rs`, tonic-web beside axum 0.8
  (`route_service` + `GrpcWebLayer`) and protox all worked first try.
- API churn was the cost: taffy 0.15's measure function now returns a whole
  `LayoutOutput`, cosmic-text 0.19 / wgpu 30 / parley 0.12 all differ from
  what one remembers. Each cost a compile round, no design change.
- A failed `Gpu.open` claims the canvas's context; the CPU fallback must draw
  on a fresh canvas.
- Under tiny-skia (the first painter, replaced), native and WebAssembly buffers
  were byte-identical — a 2× repaint took ≈220 ms in WebAssembly, which is
  the number that justified moving the canvas to the GPU.

## What the design got wrong

- **"Encodes one `vello` scene, which vello's GPU backend draws … and its CPU
  backend draws"** — not true of the crates: `vello_cpu` has its own
  `RenderContext` API and cannot replay a `vello::Scene`. What works (and is
  built) is the engine's own **display list** (`render::Draw`: fill, stroke,
  blurred shadow, glyph run, push clip/opacity, pop), laid out and shaped
  once, replayed by two thin backends. The constraint's check ("both call the
  engine's one function that builds the vello scene") needs rewording to
  "both replay the engine's one display list".
- "Plain `extern "C"` exports and a hand-written loader" (original text) is
  superseded by the amendment; the prototype uses both: wasm-bindgen for the
  GPU, raw exports still for `apply`/`click`/CPU paint — production should
  pick one (wasm-bindgen for all).
- One module instance per page holds one scene; an embedding page with
  several designs needs per-canvas state in the module, not a global.
- The canvas page's `Watch` call needs the design's address *without* a
  leading slash; the doc doesn't say.

## Left for production

- The server re-resolves a design on **every** change message, not only the
  designs made from the changed file (the watcher sends no paths today);
  no scene cache by file.
- Not built: `Vector`, `Image`, gradients, `textAlign`, `overlay`/`close`,
  zoom/pan, embedding in Markdown (`![](x.design)`), problems shown on the
  canvas page, `devpage check` reporting design problems, PDF of embeds, the
  deleted-design and reconnect cases, opacity groups measured.
- GPU vs CPU pixel tolerance not measured (no image tooling at hand); the
  constraint's test needs it — headless Chrome with `--enable-unsafe-webgpu`
  works for driving it.
- Tests: no route tests for the PNG/Watch/canvas page, no Chrome test, no
  module test; `paint_natively` in the wasm crate is a measuring hook, not a
  test. Clippy `unit_arg` allowed on the attribute setter.
- Module size: ≈7.6 MB unoptimized (fonts ≈1.7 MB); no `wasm-opt`.
- Docs not updated for the new crates (dependencies page says "no build
  script"), installing, developing (wasm target, build time), configuration
  reference (`devpage x.design`).
- Gates: fmt and clippy green; `cargo test`: the 6 file-watcher tests
  (`watch::*`, `server::the_entry_document_follows_its_file_without_a_restart`)
  fail on this machine — identically on the unchanged design branch, so they
  are environmental, not this change.
