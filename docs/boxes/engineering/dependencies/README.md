# Dependencies

Every crate devpage depends on, with the version `Cargo.lock` resolves and
what devpage uses it for. It is a reference page under
[Engineering](../README.md), for someone reading or changing the code; the
[Developing devpage](../developing-devpage/README.md#the-code) maps the modules
named here.

The list is `[dependencies]` in `Cargo.toml`; there are no
`[dev-dependencies]` and no build script. The roles below say what each crate
does in devpage. Why a crate was chosen over another is recorded nowhere,
except where a code comment says so, and this page doesn't guess.

| Crate | Locked version | Licence | What devpage uses it for | Where |
| --- | --- | --- | --- | --- |
| `axum` | 0.8.9 | MIT | The HTTP server: the router and its routes, the handlers, `Json` for the comment requests, the request-logging middleware, the server-sent event response (`axum::response::sse`) and `axum::serve` — also for the server `devpage pdf` and the Chrome tests start internally. | `src/server.rs`, `src/main.rs`, `src/pdf.rs`, `src/comments.rs` (tests) |
| `tokio` (feature `full`) | 1.54.1 | MIT | The async runtime (`#[tokio::main]`), binding the address (`TcpListener`), the `broadcast` channel that carries changes, background external-link checks, and reading files in handlers (`tokio::fs`). | `src/main.rs`, `src/server.rs`, `src/watch.rs`, `src/check.rs`, `src/pdf.rs`, `src/comments.rs` |
| `tokio-stream` (feature `sync`) | 0.1.19 | MIT | `BroadcastStream`, which turns a subscription to the change channel into the stream the event route sends. | `src/server.rs` |
| `clap` (feature `derive`) | 4.6.7 | MIT or Apache-2.0 | The command line: serving, `check` and `pdf`, their flags, `--help` and `--version`. | `src/main.rs` |
| `comrak` | 0.57.0 | BSD-2-Clause | Markdown: rendering pages to HTML with the GFM options, parsing a page to find its Topics section, and the code-fence plugin hook (`SyntaxHighlighterAdapter`) the highlighter implements. | `src/render.rs`, `src/nav.rs`, `src/highlight.rs` |
| `oxc` (features `transformer`, `codegen`, `semantic`) | 0.153.0 | MIT | Compiling widgets from TSX to browser modules: parsing, finding the widget names a file uses without declaring (`semantic`), React's automatic JSX runtime and TypeScript stripping (`transformer`), and printing the module (`codegen`). Its `rust-version = 1.97` sets devpage's minimum Rust. `Cargo.toml` pins it exactly (`=0.153.0`). | `src/widgets.rs` |
| `syntect` | 5.3.0 | MIT | Syntax highlighting: finding a language by name or extension and emitting class-based HTML (`hl-` prefixed spans). | `src/highlight.rs` |
| `two-face` | 0.5.2 (+bat-0.26.1) | MIT or Apache-2.0 | The syntax set syntect highlights with. The code comment gives the reason: it is the set `bat` ships, which includes Swift, TypeScript, TOML and YAML besides syntect's defaults. | `src/highlight.rs` |
| `notify` | 8.2.1 | CC0-1.0 | The file watcher: the platform's recommended watcher, on the served folder, recursively. | `src/watch.rs` |
| `ignore` | 0.4.33 | Unlicense or MIT | Walking the served folder to build the index while honouring every `.gitignore` in it (`WalkBuilder`), and matching changed paths against them (`Gitignore`). | `src/index.rs` |
| `globset` | 0.4.20 | Unlicense or MIT | A site's `watch` patterns: compiled into one glob set and matched against each changed path. | `src/document.rs` |
| `serde` (feature `derive`) | 1.0.229 | MIT or Apache-2.0 | Deserializing document files into the kind-tagged config with `deny_unknown_fields`, comment threads, and the comment requests' bodies. | `src/document.rs`, `src/comments.rs`, `src/server.rs` |
| `serde_yaml` | 0.9.34 (+deprecated) | MIT or Apache-2.0 | Parsing document files and writing and reading comment thread files; its messages, with line and column, are what a bad document file prints. The published version is marked `+deprecated` and its README says the project is no longer maintained. | `src/document.rs`, `src/comments.rs` |
| `serde_json` | 1.0.154 | MIT or Apache-2.0 | The comment requests' JSON answers, the list of broken links the problems bar carries for the page script to mark, and the widgets' props, widget lists and `__devpageUses`. | `src/server.rs`, `src/render.rs`, `src/widgets.rs` |
| `reqwest` | 0.13.5 | MIT or Apache-2.0 | Checking external links (one shared client, asking for HTML as a browser does, with a 10-second first wait and a 30-second retry); the route tests' HTTP client. | `src/check.rs`, `src/server.rs` (tests) |
| `futures` | 0.3.34 | MIT or Apache-2.0 | Running external-link checks at once (`join_all`) and driving chromiumoxide's event stream (`StreamExt`). | `src/check.rs`, `src/pdf.rs`, `src/comments.rs` (tests) |
| `chromiumoxide` (features `fetcher`, `rustls`, `zip8`) | 0.9.1 | MIT or Apache-2.0 | Driving Chrome over the DevTools protocol for `devpage pdf` (`Page.printToPDF`) and the Chrome tests; `fetcher` downloads a Chromium into devpage's cache folder when none is installed. | `src/pdf.rs`, `src/comments.rs` (tests) |
| `anyhow` | 1.0.104 | MIT or Apache-2.0 | Errors: `Result`, `.context(…)` to say what devpage was doing, and `bail!`. `main` returns `anyhow::Result`, so a failure prints in anyhow's format — `Error: <context>`, then `Caused by:` and the underlying errors. | most modules |
| `mime_guess` | 2.0.5 | MIT | The `Content-Type` of a file that isn't Markdown or a document file, guessed from its extension, or `application/octet-stream`. | `src/server.rs` |

Licences are as each crate's own `Cargo.toml` declares them. devpage itself
is MIT — see [Installing devpage](../../../installing.md#licence).

Three more pieces of third-party code are built into the binary but aren't
crates: the [vendored pele bundle](../../viewer/vendored-pele/README.md), the
JavaScript that draws diagrams; the [vendored React bundle](../../viewer/vendored-react/README.md),
which widgets run with; and the [vendored icons](../../viewer/vendored-icons/README.md),
Lucide's icon set (ISC) and GitHub's mark (CC0).

The error messages each failure produces are listed in the
[configuration reference](../../viewer/configuration-reference/README.md#errors).
