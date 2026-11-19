---
page-type: task
---
# Developing devpage

How to work on devpage itself: build it, run its tests and the three gates,
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
root. Use `npm run browser-tests -- --explain` to inspect which suites your
branch and working-tree changes affect. Run `npm run browser-tests` to execute
them with Cargo, or add `--full` to run the entire ignored browser binary.
Windows skips the GPU tests. A standalone review brief selects zero suites.
The command reports broad coverage when a changed file has no classification.

For the edit loop, `npm run browser-tests -- --base HEAD` selects only staged,
unstaged and untracked changes since your last commit. The default comparison
starts at the merge base with `origin/main`. Use
`--base REF` to choose another base, or `--base BASE --head HEAD` to inspect
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

Green before any pull request opens (from `AGENTS.md`):

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Formatting follows `rustfmt` defaults. Clippy checks every target, warnings
are errors, and the Rust tests must pass. CI runs these checks on Linux and
Windows. Linux also checks the docs. For browser selection, push and pull
request comparisons and the full-run controls, see
[CI](../sdlc/review-bars/README.md#ci).

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
