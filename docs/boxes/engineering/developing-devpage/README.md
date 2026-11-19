---
page-type: task
---
# Developing devpage

How to work on devpage itself: build it, run its affected checks,
run the documentation while you edit it, and know what needs a rebuild. The
[code map](../code-map/README.md) and the [tests](../tests/README.md) go one
level deeper. It sits under [Engineering](../README.md); how a change travels from idea to `main` is
the [SDLC](../sdlc/README.md).

You need the toolchain from [Installing devpage](../../../installing.md#what-you-need).
Every command runs from the repository root.

## Build and run

```sh
cargo build                 # debug build, target/debug/devpage
cargo run -- docs           # build if needed, then serve these docs
```

Building devpage builds no WebAssembly: the
[design engine](../../viewer/on-the-page/designs/design-engine/README.md)'s module for the browser
is [vendored](#the-vendored-design-engine) in `core/assets/vendor/`, and the binary
embeds it.

Run it from the repository root: the folder devpage starts from is the
[served folder](../../viewer/write/writing-a-document/README.md#the-served-folder),
so the whole repository is served and the docs' links to `../LICENSE`
work. `cargo run -- <args>` passes everything after `--` to
devpage, so `cargo run -- check docs` and `cargo run -- pdf docs/talks/devpage`
work the way an installed `devpage` would. It prints the address —
`http://localhost:3030`, or the next free port up if another devpage already
holds 3030.

### The vendored design engine

`core/assets/vendor/design-engine.wasm` and `core/assets/vendor/design-engine.js` are
the engine built for the browser and its bindings, committed like the pele
bundle. Rebuild them after changing anything they are built from:

```sh
cargo run -p devpage-design-module
```

It builds `devpage-design-wasm` for `wasm32-unknown-unknown` in the
size-optimised `wasm` profile, into `target/design-module/`, runs wasm-bindgen
on it as a library, and writes the two files and
`core/assets/vendor/design-engine.fingerprint`
([Dependencies](../dependencies/README.md#devpage-design-module-the-modules-builder)).
The target is the one `rust-toolchain.toml` names, so rustup adds it in a
checkout; nothing else needs it. `rust-toolchain.toml` also pins the Rust
version, 1.98.1, so a checkout, CI and the vendored module all build with
the same compiler: a newer `stable` on one machine can't turn a deprecation
into a clippy failure on another. The fingerprint is a SHA-256 of everything
the module is built from: every file in `design/src/`, `design/fonts/`,
`design/proto/`, `design/wasm/src/` and `design/module/src/`, the three crates'
`Cargo.toml` and `design/build.rs`, the `wasm` profile, and the locked version
of every package in the module's and wasm-bindgen's dependency trees. The
toolchain is recorded beside it but not hashed, so the same sources give the
same fingerprint on every machine. `cargo test` fails while the recorded
fingerprint differs from the sources', and says to run the command.

## The edit–save–refresh loop

What you edit decides what you have to do to see it:

| You change | What it takes |
| --- | --- |
| A Markdown page under `docs/` | Save. Pages are read on every request and the open page [updates itself](../../viewer/write/writing-a-document/live-updates/README.md). |
| A page's Topics list | Save. The sidebar is rebuilt on every request, on every open page. |
| `docs/devpage.yml`, or any document file | Save. Document files are read on every request and the index of them is kept current by the watcher. |
| Rust code in `src/` or `core/src/` | Stop devpage (Ctrl-C) and `cargo run` again, then reload the browser tab. A page reconnects to its event stream by itself, but doesn't fetch itself again on reconnecting. |
| `core/assets/style.css`, `core/assets/devpage.js`, the design script's modules (`core/assets/design*.js`), `core/assets/vendor/pele.js` | Rebuild and restart — they are built into the binary (pele by `core/src/diagrams.rs`, the rest by `core/build.rs`) — then reload the tab. Live updates and in-place links swap only the page content and its navigation, never the stylesheet or the scripts — except that a page from a new build is loaded whole, so the next live update after the restart brings the new ones. |
| A `.design` file, or an image a design uses | Save. The design store resolves again the designs made from that file and the open canvases redraw in place ([Designs](../../viewer/on-the-page/designs/README.md)). |
| The engine in `design/src/`, the module in `design/wasm/src/`, or `design/proto/design.proto` | Run `cargo run -p devpage-design-module` to rebuild the [vendored module](#the-vendored-design-engine), then rebuild and restart devpage, and do a full reload of the tab — the binary embeds the module. After changing what the engine draws, run `DEVPAGE_UPDATE_REFERENCES=1 cargo test -p devpage-design` to rewrite the reference PNGs in `design/testdata/`, and look at them before committing. |

## Tests

The default Rust suite runs with `cargo test`. Browser tests use the pinned
Chrome. Video uses Chrome for Testing. The
[test inventory](../tests/README.md) describes the behaviors exercised by
each suite.

### Select browser checks locally

Install the Node version in `.node-version` and run `npm ci` at the repository
root. Run `npm run browser-tests` during an edit. The default comparison with
`HEAD` includes staged, unstaged and untracked files. Add `-- --explain` to
see why each suite was selected. A standalone review brief selects zero
suites. An unclassified change requests broad coverage. Use `-- --full` to
run the entire ignored devpage binary. Windows skips GPU tests.

Before a PR to main, `npm run browser-tests -- --base origin/main` includes
committed branch changes since the merge base, plus working-tree changes.
Use the PR’s target branch for other branches, or `--base BASE --head HEAD` for
an explicit committed range. The latter requires BASE to be an ancestor of
HEAD. Add `--json` for a machine-readable plan. The selector also reports
whether CI should exercise the deploy-server PDF smoke. The local command
runs the Rust browser tests; the Docker smoke has its own CI runner.

When a feature starts reading a shared module, style or fixture, add it to
the relevant targets’ `inputs` in the root `project.json` or their shared
named inputs in `nx.json`. For a new Rust suite, register its libtest filters
in `metadata.browserFilters`. Exercise the selector with an edit to the new
input and assert the expected selection in the browser-tooling tests. Run
`node tools/browser-tests/inventory.mjs` to reconcile the registered filters
with the platform’s compiled tests after moving or renaming tests.

## The gates

With the repository’s pinned Node and `npm ci` at the root, use the same
affected-check entry point for a local edit and for a branch review.

| Command | Effect |
| --- | --- |
| `npm run check -- --explain` | Show what the current edit selects and why. |
| `npm run check` | Execute the selected development checks. |
| `npm run build` | Build the selected Rust targets through Cargo. |
| `npm run check -- --scope live` | Run only the affected live-session checks during a focused edit. |
| `npm exec -- nx graph` | Inspect the Nx project graph, including Cargo workspace members and their local dependencies. |
| `npm run check -- --base origin/main` | Check the branch and local edits against their merge base with main. Use the actual target for other branches. |
| `npm run check -- --full` | Request full local coverage. |

Each Cargo member has Nx build, test, Clippy and formatting targets. For
example, `npm exec -- nx run devpage-bench:build` builds that member directly.
The project check plan covers Rust formatting, Clippy and tests, docs checks, the live
session’s Bun checks, selector tests and the browser suites. An affected live
check needs the pinned Bun and `bun install --frozen-lockfile` from `live/`.
A live test edit selects TypeScript checking and that Bun test. A live eval
data edit checks that the cases can be reconstructed offline. Documentation
changes can select link checking, the requirements-to-test map, and the Rust
tests that read the edited documents. The separate Docker smoke and clean
Cargo install run in CI. Use
`-- --json` to inspect the plan or `-- --base BASE --head HEAD` to replay a
committed range. The explicit base must be an ancestor of the head.

See [CI](../sdlc/review-bars/README.md#ci) for how the platform runners use
the same selection and when the workflow requests a full run.

## The code

devpage is one binary, `src/`, built on its core, `core/` — the documents,
rendering, checking, comments and designs, with the built-in assets and the
system widgets, and no I/O runtime — and the
[design engine](../../viewer/on-the-page/designs/design-engine/README.md) in three more workspace
crates beside them, the extension ABI in `extension-api/`, and the server's
benchmark in `bench/`. Which module does what, and how `main` wires them
together, is on the [Code map](../code-map/README.md); what each crate does is
in [Dependencies](../dependencies/README.md).

## Working on the docs

These docs are a devpage site: `docs/devpage.yml`, rooted at
`docs/README.md`. Keep `cargo run -- docs` open, from the repository root,
while you write, and run `cargo run -- check docs` before you push — it runs the tests too,
because these docs hold the code to its
[constraints](../../viewer/check/checking-requirements/README.md); `check docs
--docs` checks the docs alone — `cargo
test` checks the same internal links and diagrams.
How to write and organize pages is in the
[viewer's task articles](../../viewer/README.md#topics); the page types
this repository's docs follow are in [`docs/structure/`](../../../structure/landing.md),
and `check` reports a page that breaks them. A diagram that doesn't parse is a
problem `devpage check` reports, like a broken link
([Diagrams](../../viewer/on-the-page/diagrams/README.md#checking-diagrams-before-committing)).

## Topics

- [Tests](../tests/README.md) — what the suite pins, module by module, how
  tests are written, and what has no test.
- [Code map](../code-map/README.md) — which module does what, and how `main`
  wires them together at start.
