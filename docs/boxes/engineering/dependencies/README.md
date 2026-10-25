# Dependencies

Every crate devpage depends on, with the version `Cargo.lock` resolves and
what devpage uses it for, and the build script and builder that make what it
embeds. It is a reference page under
[Engineering](../README.md), for someone reading or changing the code; the
[Developing devpage](../developing-devpage/README.md#the-code) maps the modules
named here.

devpage is a Cargo workspace of four crates: `devpage`, the binary, at the
top; `devpage-design`, the [design engine](../../viewer/design-engine/README.md),
in `design/`; `devpage-design-wasm`, the engine built for the browser, in
`design/wasm/`; and `devpage-design-module`, which builds that module into
`assets/vendor/`, in `design/module/`. `devpage` and `devpage-design` are
published; the other two are not. Each crate's list is its `[dependencies]` and
`[build-dependencies]`; there are no `[dev-dependencies]`. The roles below say
what each crate does in devpage. Why a crate was chosen over another is recorded nowhere,
except where a code comment says so, and this page doesn't guess.

| Crate | Locked version | Licence | What devpage uses it for | Where |
| --- | --- | --- | --- | --- |
| `axum` | 0.8.9 | MIT | The HTTP server: the router and its routes, the handlers, `Json` for the comment requests, the request-logging middleware, the server-sent event response (`axum::response::sse`) and `axum::serve` — also for the server `devpage pdf` and the Chrome tests start internally. | `src/server.rs`, `src/main.rs`, `src/pdf.rs`, `src/comments.rs` (tests) |
| `tokio` (feature `full`) | 1.54.1 | MIT | The async runtime (`#[tokio::main]`), binding the address (`TcpListener`), the `broadcast` channel that carries changes, background external-link checks, and reading files in handlers (`tokio::fs`). | `src/main.rs`, `src/server.rs`, `src/watch.rs`, `src/check.rs`, `src/pdf.rs`, `src/comments.rs` |
| `tokio-stream` (feature `sync`) | 0.1.19 | MIT | `BroadcastStream`, which turns a subscription to the change channel into the stream the event route sends; `ReceiverStream` and the `Stream` trait, which make the design store's updates the watch stream's response. | `src/server.rs`, `src/designs.rs` |
| `clap` (feature `derive`) | 4.6.7 | MIT or Apache-2.0 | The command line: serving, `check` and `pdf`, their flags, `--help` and `--version`. | `src/main.rs` |
| `comrak` | 0.57.0 | BSD-2-Clause | Markdown: rendering pages to HTML with the GFM options, parsing a page to find its Topics section, and the code-fence plugin hook (`SyntaxHighlighterAdapter`) the highlighter implements. | `src/render.rs`, `src/nav.rs`, `src/highlight.rs` |
| `syntect` | 5.3.0 | MIT | Syntax highlighting: finding a language by name or extension and emitting class-based HTML (`hl-` prefixed spans). | `src/highlight.rs` |
| `two-face` | 0.5.2 (+bat-0.26.1) | MIT or Apache-2.0 | The syntax set syntect highlights with. The code comment gives the reason: it is the set `bat` ships, which includes Swift, TypeScript, TOML and YAML besides syntect's defaults. | `src/highlight.rs` |
| `notify` | 8.2.1 | CC0-1.0 | The file watcher: the platform's recommended watcher, on the served folder, recursively. | `src/watch.rs` |
| `ignore` | 0.4.33 | Unlicense or MIT | Walking the served folder to build the index while honouring every `.gitignore` in it (`WalkBuilder`), and matching changed paths against them (`Gitignore`). | `src/index.rs` |
| `globset` | 0.4.20 | Unlicense or MIT | A site's `watch` patterns: compiled into one glob set and matched against each changed path. | `src/document.rs` |
| `serde` (feature `derive`) | 1.0.229 | MIT or Apache-2.0 | Deserializing document files into the kind-tagged config with `deny_unknown_fields`, comment threads, and the comment requests' bodies. | `src/document.rs`, `src/comments.rs`, `src/server.rs` |
| `serde_yaml` | 0.9.34 (+deprecated) | MIT or Apache-2.0 | Parsing document files and writing and reading comment thread files; its messages, with line and column, are what a bad document file prints. The published version is marked `+deprecated` and its README says the project is no longer maintained. | `src/document.rs`, `src/comments.rs` |
| `serde_json` | 1.0.154 | MIT or Apache-2.0 | The comment requests' JSON answers, and the list of broken links the problems bar carries for the page script to mark. | `src/server.rs`, `src/render.rs` |
| `reqwest` | 0.13.5 | MIT or Apache-2.0 | Checking external links (one shared client, asking for HTML as a browser does, 10-second timeout); the route tests' HTTP client. | `src/check.rs`, `src/server.rs` (tests) |
| `futures` | 0.3.34 | MIT or Apache-2.0 | Running external-link checks at once (`join_all`) and driving chromiumoxide's event stream (`StreamExt`). | `src/check.rs`, `src/pdf.rs`, `src/comments.rs` (tests) |
| `chromiumoxide` (features `fetcher`, `rustls`, `zip8`) | 0.9.1 | MIT or Apache-2.0 | Driving Chrome over the DevTools protocol for `devpage pdf` (`Page.printToPDF`) and the Chrome tests; `fetcher` downloads chrome-headless-shell, the only Chrome devpage drives, into devpage's cache folder the first time one is needed. | `src/pdf.rs`, `src/comments.rs` (tests) |
| `anyhow` | 1.0.104 | MIT or Apache-2.0 | Errors: `Result`, `.context(…)` to say what devpage was doing, and `bail!`. `main` returns `anyhow::Result`, so a failure prints in anyhow's format — `Error: <context>`, then `Caused by:` and the underlying errors. | most modules |
| `mime_guess` | 2.0.5 | MIT | The `Content-Type` of a file that isn't Markdown or a document file, guessed from its extension, or `application/octet-stream`. | `src/server.rs` |
| `devpage-design` (feature `server`) | 0.1.0 (path `design/`, and the same version on crates.io) | MIT | The design engine: parsing, resolving, painting a PNG, the watch stream's messages and patches — for the design store, and for `devpage check` through it — and, with `server`, the `Designs` service's server side. | `src/designs.rs`, `src/server.rs` |
| `tonic` (features `router`, `codegen`) | 0.14.6 | MIT | The gRPC server side of the watch stream: the `Designs` service's request and response types, its streaming response, and the router it is served from. | `src/designs.rs`, `src/server.rs` |
| `tonic-web` | 0.14.6 | MIT | `GrpcWebLayer`, which serves the gRPC service as gRPC-Web, so the browser can call it with `fetch` and no proxy. | `src/server.rs` |
| `tower` | 0.5.3 | MIT | `ServiceBuilder`, wrapping the gRPC service in the gRPC-Web layer before axum routes to it. | `src/server.rs` |
| `prost` | 0.14.4 | Apache-2.0 | Decoding watch-stream messages in the route tests. | `src/designs.rs` (tests) |

`devpage` has no build script and builds no WebAssembly: the binary embeds
the vendored `assets/vendor/design-engine.wasm` and
`assets/vendor/design-engine.js` as the
[built-in assets](../../viewer/serving-pages/README.md#built-in-assets)
`/__devpage/design-engine.wasm` and `/__devpage/design-engine.js`.

### `devpage-design`, the design engine

It does no I/O; it is given file contents. Its build script, `design/build.rs`,
compiles `design/proto/design.proto` with `protox` and generates the messages
with `tonic-prost-build` — and, when the `server` feature is on, the
`Designs` service's server side — so the server and the module share one set
of message types, and the proto ships inside the crate. `devpage` turns
`server` on; the module leaves it off.

| Crate | Locked version | Licence | What the engine uses it for | Where |
| --- | --- | --- | --- | --- |
| `taffy` (features `std`, `taffy_tree`, `flexbox`, `content_size`) | 0.15.1 | MIT | Auto layout: rows and columns as flexbox, hug and fill, and absolute placement. | `design/src/render.rs` |
| `parley` | 0.12.0 | Apache-2.0 or MIT | Text: shaping, measuring and line breaking over the embedded Inter faces, and the glyph runs the display list draws. | `design/src/render.rs` |
| `vello_cpu` | 0.3.0 | Apache-2.0 or MIT | The CPU backend: replays the display list into an RGBA buffer, for the PNG and for a canvas without WebGPU. | `design/src/render.rs` |
| `png` | 0.18.1 | MIT or Apache-2.0 | Encoding the PNG a design's address answers, and decoding PNG images a design uses. | `design/src/render.rs` |
| `zune-jpeg` | 0.5.15 | MIT, Apache-2.0 or Zlib | Decoding JPEG images a design uses. | `design/src/render.rs` |
| `prost` | 0.14.4 | Apache-2.0 | The watch stream's messages: encoding a scene and its patches, and decoding them in the module. | `design/src/proto.rs`, `design/src/wire.rs` |
| `tonic` (feature `codegen`; only with `server`) | 0.14.6 | MIT | What the generated `Designs` server is written against. | generated into `design/src/proto.rs` |
| `tonic-prost` (only with `server`) | 0.14.6 | MIT | The protobuf codec the generated service encodes its messages with. | generated into `design/src/proto.rs` |
| `protox` (build) | 0.10.0 | MIT or Apache-2.0 | Compiling `design/proto/design.proto` in Rust, so building needs no `protoc`. | `design/build.rs` |
| `tonic-prost-build` (build) | 0.14.6 | MIT | Generating the messages, and with `server` the service, from the descriptors `protox` compiled. | `design/build.rs` |

### `devpage-design-wasm`, the engine in the browser

A `cdylib` built only for `wasm32-unknown-unknown`, by `devpage-design-module`.

| Crate | Locked version | Licence | What the module uses it for | Where |
| --- | --- | --- | --- | --- |
| `devpage-design` | 0.1.0 (path `design/`) | MIT | The engine: applying scenes and patches, layout, the display list, the CPU backend, hit-testing. | `design/wasm/src/lib.rs` |
| `prost` | 0.14.4 | Apache-2.0 | Decoding the watch stream's messages (`Message::decode`) before they are applied to a canvas's scene. | `design/wasm/src/lib.rs` |
| `vello` | 0.11.0 | Apache-2.0 or MIT | The GPU backend: the display list replayed into a vello scene and drawn with WebGPU through `wgpu`. | `design/wasm/src/gpu.rs` |
| `wasm-bindgen` | 0.2.129 | MIT or Apache-2.0 | Every export the page calls, and the canvas and GPU handles passed in from JavaScript. Pinned to the version `wasm-bindgen-cli-support` generates bindings for. | `design/wasm/src/lib.rs`, `design/wasm/src/gpu.rs` |
| `wasm-bindgen-futures` | 0.4.79 | MIT or Apache-2.0 | Exporting `Gpu::open`, which awaits WebGPU's adapter and device, as an `async` function the page receives as a promise. | `design/wasm/src/gpu.rs` |
| `web-sys` (feature `HtmlCanvasElement`) | 0.3.106 | MIT or Apache-2.0 | The `<canvas>` element a GPU target draws into. | `design/wasm/src/gpu.rs` |

### `devpage-design-module`, the module's builder

`cargo run -p devpage-design-module` builds `devpage-design-wasm` for
`wasm32-unknown-unknown` with a nested `cargo build --profile wasm --locked`
into `target/design-module/`, runs wasm-bindgen on the result, and writes the
module and its bindings into `assets/vendor/` with
`design-engine.fingerprint`. The `wasm` profile in the top `Cargo.toml`
inherits `release` and optimises for size (`opt-level = "s"`, `lto`, one
codegen unit, stripped), because the page downloads the module. Its test
fails when the fingerprint no longer matches the sources
([Developing devpage](../developing-devpage/README.md#the-vendored-design-engine)).

| Crate | Locked version | Licence | What the builder uses it for | Where |
| --- | --- | --- | --- | --- |
| `wasm-bindgen-cli-support` | 0.2.129 | MIT or Apache-2.0 | wasm-bindgen as a library: the module and its JavaScript bindings, with no CLI to install. Pinned to exactly the `wasm-bindgen` version the module uses, as wasm-bindgen requires. | `design/module/src/main.rs` |
| `sha2` | 0.10.9 | MIT or Apache-2.0 | The fingerprint: a SHA-256 of everything the module is built from. | `design/module/src/lib.rs` |
| `toml` | 0.9.12 (+spec-1.1.0) | MIT or Apache-2.0 | Reading `Cargo.lock` for the module's locked dependency tree, and the `wasm` profile from `Cargo.toml`. | `design/module/src/lib.rs` |

Licences are as each crate's own `Cargo.toml` declares them. devpage itself
is MIT — see [Installing devpage](../../../installing.md#licence).

Two more pieces of third-party work are built into the binary but aren't
crates: the [vendored pele bundle](../../viewer/vendored-pele/README.md), the
JavaScript that draws diagrams, and the Inter typeface — regular, medium,
semibold and bold, in `design/fonts/` — which the design engine embeds with
`include_bytes!` and sets every design's text in. Inter is under the SIL Open
Font License 1.1, whose text ships beside the fonts as
`design/fonts/LICENSE.txt`.

The error messages each failure produces are listed in the
[configuration reference](../../viewer/configuration-reference/README.md#errors).
