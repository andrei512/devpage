# Dependencies

Every crate devpage depends on, with the version `Cargo.lock` resolves and
what devpage uses it for, and the build scripts and builder that make what it
embeds. It is a reference page under
[Engineering](../README.md), for someone reading or changing the code; the
[code map](../code-map/README.md) maps the modules named here.

devpage is a Cargo workspace of four crates: `devpage`, the binary, at the
top; `devpage-design`, the [design engine](../../viewer/design-engine/README.md),
in `design/`; `devpage-design-wasm`, the engine built for the browser, in
`design/wasm/`; and `devpage-design-module`, which builds that module into
`assets/vendor/`, in `design/module/`. `devpage` and `devpage-design` are the
two that are published, together, at a release
([Release](../sdlc/stages/README.md#release)); the other two never are. Each
crate's list is its `[dependencies]` and `[build-dependencies]`, and
`devpage`'s `[dev-dependencies]`, marked below. The roles below say
what each crate does in devpage. Why a crate was chosen over another is recorded nowhere,
except where a code comment says so, and this page doesn't guess.

## The server

| Crate | Locked version | Licence | What devpage uses it for | Where |
| --- | --- | --- | --- | --- |
| `axum` | 0.8.9 | MIT | The HTTP server: the router and its routes, the handlers, `Json` for the comment requests, the request-logging middleware, the server-sent event response (`axum::response::sse`) and `axum::serve` — also for the server `devpage pdf` and the Chrome tests start internally. | `src/server.rs`, `src/main.rs`, `src/pdf.rs`, `src/comments.rs` (tests) |
| `tokio` (feature `full`) | 1.54.1 | MIT | The async runtime (`#[tokio::main]`), binding the address (`TcpListener`), the `broadcast` channel that carries changes, background external-link checks, and reading files in handlers (`tokio::fs`). | `src/main.rs`, `src/server.rs`, `src/watch.rs`, `src/check.rs`, `src/pdf.rs`, `src/comments.rs` |
| `tokio-stream` (feature `sync`) | 0.1.19 | MIT | `BroadcastStream`, which turns a subscription to the change channel into the stream the event route sends; `ReceiverStream` and the `Stream` trait, which make the design store's updates the watch stream's response. | `src/server.rs`, `src/designs.rs` |
| `clap` (feature `derive`) | 4.6.7 | MIT or Apache-2.0 | The command line: serving, `check` and `pdf`, their flags, `--help` and `--version`. | `src/main.rs` |

## Markdown and code

| Crate | Locked version | Licence | What devpage uses it for | Where |
| --- | --- | --- | --- | --- |
| `comrak` | 0.57.0 | BSD-2-Clause | Markdown: rendering pages to HTML with the GFM options, parsing a page to find its Topics section, and the code-fence plugin hook (`SyntaxHighlighterAdapter`) the highlighter implements. | `src/render.rs`, `src/nav.rs`, `src/highlight.rs` |
| `syntect` | 5.3.0 | MIT | Syntax highlighting: finding a language by name or extension and emitting class-based HTML (`hl-` prefixed spans). | `src/highlight.rs` |
| `two-face` | 0.5.2 (+bat-0.26.1) | MIT or Apache-2.0 | The syntax set syntect highlights with. The code comment gives the reason: it is the set `bat` ships, which includes Swift, TypeScript, TOML and YAML besides syntect's defaults. | `src/highlight.rs` |

## Widgets

| Crate | Locked version | Licence | What devpage uses it for | Where |
| --- | --- | --- | --- | --- |
| `oxc` (features `transformer`, `codegen`, `semantic`) | 0.153.0 | MIT | Compiling widgets from TSX to browser modules: parsing, finding the widget names a file uses without declaring (`semantic`), React's automatic JSX runtime and TypeScript stripping (`transformer`), and printing the module (`codegen`). Its `rust-version = 1.97` sets devpage's minimum Rust. `Cargo.toml` pins it exactly (`=0.153.0`). | `src/widgets.rs` |

## Diagrams

| Crate | Locked version | Licence | What devpage uses it for | Where |
| --- | --- | --- | --- | --- |
| `rquickjs` | 0.14.0 | MIT | The [diagram engine](../../viewer/diagrams/README.md#the-diagram-engine): a QuickJS runtime and context running the vendored pele bundle, with a memory limit, a maximum stack size and an interrupt handler that stops a drawing past its time. Its `rquickjs-sys` builds QuickJS-NG 0.16.2 from the C source it ships, with bindings generated ahead for the common targets, so building devpage needs a C compiler and no system library; `Cargo.toml` builds it optimised in development builds too. | `src/diagrams.rs` |

## Files, watching and search

| Crate | Locked version | Licence | What devpage uses it for | Where |
| --- | --- | --- | --- | --- |
| `notify` | 8.2.1 | CC0-1.0 | The file watcher on Linux — its inotify watcher, non-recursive on every folder in sight — and, everywhere but macOS and Linux, its recommended watcher on the whole served folder; its event type is what every platform's watcher reports, on macOS too. | `src/watch.rs`, `src/watch/inotify.rs`, `src/watch/recursive.rs` |
| `objc2-core-services` (macOS only; features `FSEvents`, `dispatch2`) | 0.3.2 | Zlib or Apache-2.0 or MIT | The FSEvents binding: an FSEvents stream per deep folder, on a dispatch queue. `notify`'s own FSEvents watcher can't leave a subtree out (it filters in its callback, after fseventsd has delivered every event), so devpage drives FSEvents itself. Already in the tree through `notify`. | `src/watch/macos.rs` |
| `objc2-core-foundation` (macOS only; features `CFArray`, `CFString`) | 0.3.2 | Zlib or Apache-2.0 or MIT | The array of one path an FSEvents stream is created with. Already in the tree through `notify`. | `src/watch/macos.rs` |
| `dispatch2` (macOS only) | 0.3.1 | Zlib or Apache-2.0 or MIT | The dispatch queue the FSEvents streams call back on, so no run-loop thread is needed. Already in the tree through `objc2-core-services`. | `src/watch/macos.rs` |
| `libc` (macOS only) | 0.2.191 | MIT or Apache-2.0 | kqueue (`EVFILT_VNODE`) on the shallow folders and the files directly in them, and raising the open-files limit kqueue's descriptors count against. Already in the tree. | `src/watch/macos.rs` |
| `ignore` | 0.4.33 | Unlicense or MIT | Walking the served folder to build the index while honouring every `.gitignore` in it (`WalkBuilder`), and matching changed paths against them (`Gitignore`). | `src/index.rs` |
| `globset` | 0.4.20 | Unlicense or MIT | A site's `watch` patterns: compiled into one glob set and matched against each changed path. | `src/document.rs` |
| `tantivy` (default features off; `stemmer`) | 0.27.0 | MIT | The [search index](../../viewer/search/README.md): an index in memory, its schema, tokenizers (`en_stem`, `default`, `raw`, and `code` built from its whitespace tokenizer and lowercaser) and fast fields, the writer, BM25, term, phrase (`PhraseQuery`), near (`FuzzyTermQuery`), prefix, dis-max and constant-score queries, and the top sections with their scores tweaked by structure (`TopDocs::tweak_score`). The code comment gives the reason: a full-text engine as a library in pure Rust, with nothing to run beside devpage and nothing written to disk. | `src/search.rs` |
| `mime_guess` | 2.0.5 | MIT | The `Content-Type` of a file that isn't Markdown or a document file, guessed from its extension, or `application/octet-stream`. | `src/server.rs` |

## Documents, comments and data

| Crate | Locked version | Licence | What devpage uses it for | Where |
| --- | --- | --- | --- | --- |
| `serde` (feature `derive`) | 1.0.229 | MIT or Apache-2.0 | Deserializing document files into the kind-tagged config with `deny_unknown_fields`, comment threads, and the comment requests' bodies. | `src/document.rs`, `src/comments.rs`, `src/server.rs` |
| `serde_yaml` | 0.9.34 (+deprecated) | MIT or Apache-2.0 | Parsing document files and writing and reading comment thread files; its messages, with line and column, are what a bad document file prints. The published version is marked `+deprecated` and its README says the project is no longer maintained. | `src/document.rs`, `src/comments.rs` |
| `serde_json` | 1.0.154 | MIT or Apache-2.0 | The comment requests' JSON answers, the list of broken links the problems bar carries for the page script to mark, and the widgets' props, widget lists and `__devpageUses`. | `src/server.rs`, `src/render.rs`, `src/widgets.rs` |

## Links and Chrome

| Crate | Locked version | Licence | What devpage uses it for | Where |
| --- | --- | --- | --- | --- |
| `reqwest` | 0.13.5 | MIT or Apache-2.0 | Checking external links (one shared client, asking for HTML as a browser does, with a 10-second first wait and a 30-second retry); the route tests' HTTP client. | `src/check.rs`, `src/server.rs` (tests) |
| `futures` | 0.3.34 | MIT or Apache-2.0 | Running external-link checks at once (`join_all`) and driving chromiumoxide's event stream (`StreamExt`). | `src/check.rs`, `src/pdf.rs`, `src/comments.rs` (tests), `src/designs/canvas_tests.rs` |
| `chromiumoxide` (features `fetcher`, `rustls`, `zip8`) | 0.9.1 | MIT or Apache-2.0 | Driving Chrome over the DevTools protocol for `devpage pdf` (`Page.printToPDF`), a slide's PNG (`Page.captureScreenshot`), `devpage video` (evaluating its recorder, pressing →, sizing the window) and the Chrome tests; `fetcher` downloads chrome-headless-shell — and, for `devpage video` only, Chrome for Testing — at the pinned `CHROME_VERSION` into devpage's cache folder the first time one is needed. Whoever upgrades chromiumoxide bumps `CHROME_VERSION` in the same change, to a Chrome for Testing build that speaks its DevTools protocol, and runs the Chrome tests. | `src/pdf.rs`, `src/slide_png.rs`, `src/video.rs`, `src/comments.rs` (tests), `src/designs/canvas_tests.rs` |

## Video

| Crate | Locked version | Licence | What devpage uses it for | Where |
| --- | --- | --- | --- | --- |
| `async-tungstenite` (features `tokio-runtime`, `tokio-rustls-native-certs`) | 0.32.1 | MIT | The WebSocket to GPT-Live for `devpage video`, over rustls with the system's root certificates. chromiumoxide already speaks DevTools through it, so it adds no crate. | `src/video/live.rs` |
| `base64` | 0.22.1 | MIT or Apache-2.0 | The voice's PCM in GPT-Live's events and on its way into the page, and the finished mp4 on its way back out. | `src/video.rs`, `src/video/live.rs` |

## Designs

| Crate | Locked version | Licence | What devpage uses it for | Where |
| --- | --- | --- | --- | --- |
| `devpage-design` (feature `server`) | 0.2.1 (path `design/`, published beside devpage; 0.2.1 is its first release that links no fontconfig, so devpage requires it) | MIT | The design engine: parsing, resolving, painting a PNG, the watch stream's messages and patches — for the design store, and for `devpage check` through it — and, with `server`, the `Designs` service's server side. | `src/designs.rs`, `src/server.rs` |
| `tonic` (feature `codegen`) | 0.14.6 | MIT | The gRPC server side of the watch stream: the `Designs` service's request and response types and its streaming response. axum routes to the service itself, so tonic's own router is not used. | `src/designs.rs`, `src/server.rs` |
| `tonic-web` | 0.14.6 | MIT | `GrpcWebLayer`, which serves the gRPC service as gRPC-Web, so the browser can call it with `fetch` and no proxy. | `src/server.rs` |
| `tower` | 0.5.3 | MIT | `ServiceBuilder`, wrapping the gRPC service in the gRPC-Web layer before axum routes to it. | `src/server.rs` |
| `prost` (dev) | 0.14.4 | Apache-2.0 | Decoding watch-stream messages in the route tests. | `src/designs.rs` (tests) |

## Caching

| Crate | Locked version | Licence | What devpage uses it for | Where |
| --- | --- | --- | --- | --- |
| `sha2` | 0.10.9 | MIT or Apache-2.0 | Content addresses and validators: the hash in a built-in asset's address and devpage's build id (build), a widget's version, and the ETag of a response or a design's scene. | `build.rs`, `src/addresses.rs`, `src/widgets.rs`, `src/validators.rs` |
| `brotli` (build, dev) | 9.1.0 | BSD-3-Clause and MIT | Compressing each built-in asset as brotli when devpage is built; decoding it in the route tests. | `build.rs`, `src/server.rs` (tests) |
| `flate2` (build, dev) | 1.1.10 | MIT or Apache-2.0 | The same, as gzip. | `build.rs`, `src/server.rs` (tests) |

`devpage`'s build script, `build.rs`, makes the asset manifest: each
[built-in asset](../../viewer/serving-pages/README.md#built-in-assets), its
references to other assets rewritten to their content addresses, hashed,
and compressed once into `OUT_DIR` — a compressed file is named by its
content's hash, so it is made again only when the asset changes, and the
files of an asset's earlier versions are removed — and devpage's build id, a hash of everything the binary is built from.
`src/addresses.rs` includes the manifest. devpage builds no WebAssembly: the
binary embeds the vendored `assets/vendor/design-engine.wasm` and
`assets/vendor/design-engine.js` as the built-in assets `design-engine.wasm`
and `design-engine.js`.

## Errors

| Crate | Locked version | Licence | What devpage uses it for | Where |
| --- | --- | --- | --- | --- |
| `anyhow` | 1.0.104 | MIT or Apache-2.0 | Errors: `Result`, `.context(…)` to say what devpage was doing, and `bail!`. `main` returns `anyhow::Result`, so a failure prints in anyhow's format — `Error: <context>`, then `Caused by:` and the underlying errors. | most modules |

## The design engine's crates

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
| `parley` (feature `std` only) | 0.12.0 | Apache-2.0 or MIT | Text: shaping, measuring and line breaking over the embedded Inter faces and a scene's font files, and the glyph runs the display list draws; through its `fontique`, reading a font file's family, weights and weight axis to check it. Its default `system` feature is off: the engine never loads system fonts, and with it on, building on Linux would need fontconfig. | `design/src/render.rs`, `design/src/fonts.rs` |
| `vello_cpu` | 0.3.0 | Apache-2.0 or MIT | The CPU backend: replays the display list into an RGBA buffer, for the PNG and for a canvas without WebGPU; samples a shadow's shape before the painter blurs it. | `design/src/render.rs` |
| `png` | 0.18.1 | MIT or Apache-2.0 | Decoding PNG images a design uses, while resolving; encoding test images. | `design/src/resolve.rs`, `design/src/render.rs` (tests) |
| `zune-jpeg` | 0.5.15 | MIT, Apache-2.0 or Zlib | Decoding JPEG images a design uses, while resolving, strictly, so a truncated file is a problem. | `design/src/resolve.rs` |
| `prost` | 0.14.4 | Apache-2.0 | The watch stream's messages: encoding a scene and its patches, and decoding them in the module. | `design/src/proto.rs`, `design/src/wire.rs` |
| `tonic` (feature `codegen`; only with `server`) | 0.14.6 | MIT | What the generated `Designs` server is written against. | generated into `design/src/proto.rs` |
| `tonic-prost` (only with `server`) | 0.14.6 | MIT | The protobuf codec the generated service encodes its messages with. | generated into `design/src/proto.rs` |
| `protox` (build) | 0.10.0 | MIT or Apache-2.0 | Compiling `design/proto/design.proto` in Rust, so building needs no `protoc`. | `design/build.rs` |
| `tonic-prost-build` (build) | 0.14.6 | MIT | Generating the messages, and with `server` the service, from the descriptors `protox` compiled. | `design/build.rs` |

### `devpage-design-wasm`, the engine in the browser

A `cdylib` built only for `wasm32-unknown-unknown`, by `devpage-design-module`.

| Crate | Locked version | Licence | What the module uses it for | Where |
| --- | --- | --- | --- | --- |
| `devpage-design` | 0.2.1 (path `design/`) | MIT | The engine: applying scenes and patches, layout, the display list, the CPU backend, hit-testing. | `design/wasm/src/lib.rs` |
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

## Licences and code that isn't a crate

Licences are as each crate's own `Cargo.toml` declares them. devpage itself
is MIT — see [Installing devpage](../../../installing.md#licence).

Four more pieces of third-party work are built into the binary but aren't
crates: the [vendored pele bundle](../../viewer/vendored-pele/README.md), the
JavaScript that draws diagrams; the [vendored React bundle](../../viewer/vendored-react/README.md),
which widgets run with; the [vendored icons](../../viewer/vendored-icons/README.md),
Lucide's icon set (ISC) and GitHub's mark (CC0); and the Inter typeface —
regular, medium, semibold and bold, in `design/fonts/` — which the design
engine embeds with `include_bytes!` and sets a design's text in unless the
design names a [font file](../../viewer/designs/README.md#fonts). Inter is
under the SIL Open Font License 1.1, whose text ships beside the fonts as
`design/fonts/LICENSE.txt`.

The docs' checkout example sets its text in two variable fonts, Lora and
Figtree, in `docs/designs/checkout/fonts/`. They are part of the docs, not of
the binary, and are under the SIL Open Font License 1.1 too, whose text is
beside each as `OFL.txt`.

The error messages each failure produces are listed in the
[configuration reference](../../viewer/configuration-reference/README.md#errors).
