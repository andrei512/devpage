# Developing devpage

How to work on devpage itself: build it, run its tests and the three gates,
run the documentation while you edit it, and know what needs a rebuild. The
[code map](../code-map/README.md) and the [tests](../tests/README.md) go one
level deeper. It is a task article
under [Engineering](../README.md); how a change travels from idea to `main` is
the [SDLC](../sdlc/README.md).

You need the toolchain from [Installing devpage](../../../installing.md#what-you-need).
Every command runs from the repository root.

## Build and run

```sh
cargo build                 # debug build, target/debug/devpage
cargo run -- docs           # build if needed, then serve these docs
```

Building devpage builds no WebAssembly: the
[design engine](../../viewer/design-engine/README.md)'s module for the browser
is [vendored](#the-vendored-design-engine) in `assets/vendor/`, and the binary
embeds it.

Run it from the repository root: the folder devpage starts from is the
[served folder](../../viewer/writing-a-document/README.md#the-served-folder),
so the whole repository is served and the docs' links to `../LICENSE`
work. `cargo run -- <args>` passes everything after `--` to
devpage, so `cargo run -- check docs` and `cargo run -- pdf docs/talks/devpage`
work the way an installed `devpage` would. It prints the address —
`http://localhost:3030`, or the next free port up if another devpage already
holds 3030.

### The vendored design engine

`assets/vendor/design-engine.wasm` and `assets/vendor/design-engine.js` are
the engine built for the browser and its bindings, committed like the pele
bundle. Rebuild them after changing anything they are built from:

```sh
cargo run -p devpage-design-module
```

It builds `devpage-design-wasm` for `wasm32-unknown-unknown` in the
size-optimised `wasm` profile, into `target/design-module/`, runs wasm-bindgen
on it as a library, and writes the two files and
`assets/vendor/design-engine.fingerprint`
([Dependencies](../dependencies/README.md#devpage-design-module-the-modules-builder)).
The target is the one `rust-toolchain.toml` names, so rustup adds it in a
checkout; nothing else needs it. The fingerprint is a SHA-256 of everything
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
| A Markdown page under `docs/` | Save. Pages are read on every request and the open page [updates itself](../../viewer/live-updates/README.md). |
| A page's Topics list | Save. The sidebar is rebuilt on every request, on every open page. |
| `docs/devpage.yml`, or any document file | Save. Document files are read on every request and the index of them is kept current by the watcher. |
| Rust code in `src/` | Stop devpage (Ctrl-C) and `cargo run` again, then reload the browser tab. A page reconnects to its event stream by itself, but doesn't fetch itself again on reconnecting. |
| `assets/style.css`, `assets/devpage.js`, `assets/design.js`, `assets/vendor/pele.js` | Rebuild and restart — they are compiled into the binary with `include_str!` — then do a full reload of the tab. Live updates and in-place links swap only the page content and its navigation, never the stylesheet or the scripts. |
| A `.design` file, or an image a design uses | Save. The design store resolves again the designs made from that file and the open canvases redraw in place ([Designs](../../viewer/designs/README.md)). |
| The engine in `design/src/`, the module in `design/wasm/src/`, or `design/proto/design.proto` | Run `cargo run -p devpage-design-module` to rebuild the [vendored module](#the-vendored-design-engine), then rebuild and restart devpage, and do a full reload of the tab — the binary embeds the module. After changing what the engine draws, run `DEVPAGE_UPDATE_REFERENCES=1 cargo test -p devpage-design` to rewrite the reference PNGs in `design/testdata/`, and look at them before committing. |

## Tests

<Terminal>

```text
$ cargo test                  # all four crates; needs no network and no Chrome
$ cargo test -- --ignored     # the tests that drive Chrome
```

</Terminal>

Run the Chrome tests after changing the page script, the design script, the
module, the stylesheet, a widget, print mode, `devpage pdf` or `devpage video`.
They drive chrome-headless-shell — `devpage video`'s, Chrome for Testing —
never a Chrome installed on the machine. What each module's tests pin, the watcher tests
that need file-system events, and what has no test, is on
[Tests](../tests/README.md).

## The gates

Green before any pull request opens (from `AGENTS.md`):

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Formatting must match `rustfmt`'s defaults, clippy's warnings — on tests too —
are errors, and the suite passes. There is no CI yet, so nothing runs these
for you: run them by hand before you push. See
[CI](../sdlc/review-bars/README.md#ci).

## The code

devpage is one binary — `main.rs` and its modules — with its built-in assets
and the system widgets, and the [design engine](../../viewer/design-engine/README.md)
in three workspace crates beside it. Which module does what, and how `main` wires them
together, is on the [Code map](../code-map/README.md); what each crate does is
in [Dependencies](../dependencies/README.md).

## Working on the docs

These docs are a devpage site: `docs/devpage.yml`, rooted at
`docs/README.md`. Keep `cargo run -- docs` open, from the repository root,
while you write, and run `cargo run -- check docs` before you push — `cargo
test` checks the same internal links.
How to write and organize pages is in the
[viewer's task articles](../../viewer/README.md#topics); the reading hierarchy
this repository follows is the documentation skill,
`.claude/skills/documentation/SKILL.md`. Check every diagram you add with the
Node command in [Diagrams](../../viewer/diagrams/README.md#checking-a-diagram-before-committing).

## Topics

- [Tests](../tests/README.md) — what the suite pins, module by module, how
  tests are written, and what has no test.
- [Code map](../code-map/README.md) — which module does what, and how `main`
  wires them together at start.
