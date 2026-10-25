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
| `axum` | 0.8.9 | MIT | The HTTP server: the router and its four routes, the handlers, the request-logging middleware, the server-sent event response (`axum::response::sse`) and `axum::serve`. | `src/server.rs`, `src/main.rs` |
| `tokio` (feature `full`) | 1.54.1 | MIT | The async runtime (`#[tokio::main]`), binding the port (`TcpListener`), the `broadcast` channel that carries changes, and reading files in handlers (`tokio::fs`). | `src/main.rs`, `src/server.rs`, `src/watch.rs` |
| `tokio-stream` (feature `sync`) | 0.1.19 | MIT | `BroadcastStream`, which turns a subscription to the change channel into the stream the event route sends. | `src/server.rs` |
| `comrak` | 0.57.0 | BSD-2-Clause | Markdown: rendering a page to HTML with the GFM options, parsing a page to find its Topics section, and the code-fence plugin hook (`SyntaxHighlighterAdapter`) the highlighter implements. | `src/render.rs`, `src/nav.rs`, `src/highlight.rs` |
| `syntect` | 5.3.0 | MIT | Syntax highlighting: finding a language by name or extension and emitting class-based HTML (`hl-` prefixed spans). | `src/highlight.rs` |
| `two-face` | 0.5.2 (+bat-0.26.1) | MIT or Apache-2.0 | The syntax set syntect highlights with. The code comment gives the reason: it is the set `bat` ships, which includes Swift, TypeScript, TOML and YAML besides syntect's defaults. | `src/highlight.rs` |
| `notify` | 8.2.1 | CC0-1.0 | The file watcher: the platform's recommended watcher, on the document's folder, recursively. | `src/watch.rs` |
| `globset` | 0.4.20 | Unlicense or MIT | The `watch` patterns: compiled into one glob set and matched against each changed path. | `src/document.rs` |
| `serde` (feature `derive`) | 1.0.229 | MIT or Apache-2.0 | Deserializing `devpage.yml` into `Config`, with `deny_unknown_fields` and the `watch` default. | `src/document.rs` |
| `serde_yaml` | 0.9.34 (+deprecated) | MIT or Apache-2.0 | Parsing the YAML; its messages, with line and column, are what a bad `devpage.yml` prints. The published version is marked `+deprecated` and its README says the project is no longer maintained. | `src/document.rs` |
| `anyhow` | 1.0.104 | MIT or Apache-2.0 | Errors: `Result`, `.context(…)` to say what devpage was doing, and `bail!`. `main` returns `anyhow::Result`, so a failure prints in anyhow's format — `Error: <context>`, then `Caused by:` and the underlying errors. | `src/main.rs`, `src/document.rs`, `src/watch.rs` |
| `mime_guess` | 2.0.5 | MIT | The `Content-Type` of a file that isn't Markdown, guessed from its extension, or `application/octet-stream`. | `src/server.rs` |

Licences are as each crate's own `Cargo.toml` declares them. devpage itself
declares none — see [Installing devpage](../../../installing.md#licence).

One more piece of third-party code is built into the binary but isn't a crate:
the [vendored pele bundle](../../viewer/vendored-pele/README.md), the
JavaScript that draws diagrams.

The error messages each failure produces are listed in the
[configuration reference](../../viewer/configuration-reference/README.md#errors).
