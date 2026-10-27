# Self-hosting plan

devpage deployable two ways — **bundled** (a repository's documents in an
image) and **server mode** (`devpage server`: documents published one at a
time, every version kept). The design of server mode is in
[docs/boxes/viewer/server/](docs/boxes/viewer/server/README.md) on this branch.

Two stages, in order: first reshape today's code without changing what it
does, then build server mode on the new seams.

## Stage 1 — Refactor: today's behaviour, a better shape

Branch `design/hexagonal`, in its own worktree, cut from latest `main`.
Nothing a user sees changes. Every step keeps the gates green
(`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo test`, and `cargo test -- --ignored` where Chrome is involved), moves
tests to the new interface and deletes the ones it replaces, and updates the
code map and box docs it touches.

The shape: a **core** with no tokio, axum or notify; **ports** only where two
adapters really vary — comments, changes, the external link checker;
**adapters** for the disk, YAML, the watcher, HTTP, the CLI and Chrome. Start
as modules in one crate (`domain/`, `ports/`, `adapters/`, `drive/`); split the
core into its own crate once step 1 has removed the cycle, so the compiler
holds the line.

| # | Step | What changes | Done when |
| --- | --- | --- | --- |
| 1 | **Make rendering pure** | A leaf module for `Problem`, title and notes parsing breaks the `check` ↔ `render` cycle. Internal link checking becomes pure; the external-link checker becomes an injected port (HTTP adapter + test fake). The four globals in `check.rs` go. | Rendering a page starts no network request; render tests use no network. |
| 2 | **One served-folder value** | `ServedFolder` owns resolve-inside, read, list and `url()` with a mount prefix, replacing ~10 hand-written copies of join/canonicalize/starts_with. A value, not a trait. | No module outside it canonicalizes a document path. |
| 3 | **A request resolver** | `resolve(served folder, url path, mount prefix) → Answer` (Page, Redirect, Bytes, DesignPng, SlidePng, NotFound) leaves `server.rs`; axum only maps an answer to a response. The four router constructors become one builder. | Every page route is tested through `resolve` without a socket. |
| 4 | **Split `widgets.rs`** | One module each for the widget set, Markdown tags (pure), TSX compile, and the cache — owned, not global. | No global cache; the Markdown tag preprocessor is tested on strings. |
| 5 | **A typed change bus** | `Change { document, Refresh \| Comments(page) }` replaces the global `String` channel. The watcher is one producer; its cache invalidation is its own listener, not part of the bus. | The page subscribes to its document's changes only; `check` holds no channel. |
| 6 | **A comment store port** | `CommentStore` (`list`, `create`, `reply`, `resolve`, scoped to a document) with the YAML adapter as today and an in-memory adapter for tests. Each write emits its own change; YAML-parse problems move into the YAML adapter. | The comment HTTP flow is tested with the in-memory adapter; the watcher no longer carries comment events. |
| 7 | **The design store without gRPC** | The tonic `Watch` service and the canvas page move to the HTTP side; the design store keeps its `Files` port. | `designs.rs`'s domain part depends on no tonic. |

Deferred until stage 2 shows a need: one `Index::owner(path)` lookup instead
of reloading documents in four modules.

## Stage 2 — Build server mode

Branch `design/self-hosting`, rebased onto stage 1. Each step is designed in
[docs/boxes/viewer/server/](docs/boxes/viewer/server/README.md).

1. **Configuration from the environment** — every flag readable from its
   `DEVPAGE_*` variable; strict checking at startup.
2. **Versions on disk** — a version zip unpacked into the cache as a
   `ServedFolder` with mount prefix `/d/<id>/v/<n>`, rendered by the stage 1
   resolver.
3. **The store** — SQLite with `documents`, `versions`, `threads`, `messages`;
   the SQLite adapter of the comment store port; re-anchoring on publish.
4. **The bucket and the cache** — S3 and Azure Blob through `object_store`;
   the bounded, least-recently-used cache on the PVC.
5. **`devpage server`** — the API (create, version, list, delete, comments),
   the pages (`/`, `/d/<id>`, `/d/<id>/v/<n>`), live updates on publish through
   the change bus, PDF and PNG downloads.
6. **The CLI** — contexts with `token-command`; `publish` (zip built from the
   check graph), `list`, `versions`, `comments`, `download`, `delete`, all
   with `--json`.
7. **Deploy examples** — `deploy/bundled/` and `deploy/server/`: Dockerfiles,
   `litestream.yml`, compose files with Azurite and an S3 emulator; the
   bucket and restore tests run against both as `--ignored` tests.
8. **Docs and deck** — the bundled deploy page and configuration reference
   updated; a showcase deck slide for server mode.

## Out of scope

Live features (records, page ↔ agent events), a hosted offering, sign-in or
SSO, a GitHub App, more than one replica, and any database but SQLite.
