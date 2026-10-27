# Installing devpage

How to get a `devpage` program you can run on any document: installing it from
crates.io, what `devpage pdf` and `devpage video` need, building it from a checkout instead, and
starting it. It is a task page under the [devpage landing page](README.md);
[Getting started](getting-started.md) then walks a first site, doc and deck.

## What you need

- **Rust 1.97 or later**, with `cargo` — devpage is installed by building it.
  Check what you have with `cargo --version`; install or update Rust with
  [rustup](https://rustup.rs) (`rustup update`).
- **Nothing for `devpage pdf`.** It never uses a Chrome installed on the
  machine: the first time it prints, it downloads chrome-headless-shell, at
  the version devpage pins, once into devpage's cache folder and uses that
  from then on, with no network
  ([Printing to PDF](boxes/viewer/pdf/README.md#the-chrome-it-drives)).
- **A GPT-Live key for `devpage video`.** It narrates a deck with GPT-Live on
  OpenAI or Azure, chosen by `DEVPAGE_` environment variables, and the first
  time it records it downloads Chrome for Testing, at the same pinned
  version, once into the same cache folder ([Making a video](boxes/viewer/making-a-video/README.md#before-you-run-it)).

<Callout kind="note" title="Why 1.97">
The locked `oxc 0.153.0`, which compiles widgets, declares
`rust-version = 1.97`, the highest of any dependency.
</Callout>

<Callout kind="note" title="Linux, releases up to 0.2.0">
Releases up to 0.2.0 link fontconfig, so installing one on Linux also needs
`pkg-config` and fontconfig's headers — on Debian or Ubuntu,
`apt install pkg-config libfontconfig1-dev`. Later releases and a checkout
need neither.
</Callout>

Nothing else is needed — no `wasm32-unknown-unknown` target, no `protoc`,
no wasm-bindgen CLI. The stylesheet, the page script, the diagram renderer and
the [design engine](boxes/viewer/design-engine/README.md)'s WebAssembly module
are compiled into the binary; the module is
[vendored](boxes/viewer/design-engine/README.md#one-crate-two-targets), so
installing devpage builds no WebAssembly. Diagrams are drawn by the reader's
browser, and designs by the module. The engine's protobuf file is compiled
with a Rust library
([Dependencies](boxes/engineering/dependencies/README.md#devpage-design-the-design-engine)).
The first build downloads the crates `devpage` and `devpage-design` use and
compiles the engine once, for your machine.

The result is one file. A release build of `devpage` for 64-bit ARM macOS is
about 40 MB, of which the design engine's module is 4.9 MB (1.9 MB gzipped, as
a compressing proxy in front of a deployed devpage would send it; devpage
itself sends it uncompressed).

## Install it

<Tabs>
<Tab title="From crates.io">

```sh
cargo install devpage --locked
```

This builds devpage from crates.io with the dependency versions it was
released with, and puts the binary in cargo's `bin` folder — `~/.cargo/bin`
unless `CARGO_HOME` or `CARGO_INSTALL_ROOT` points elsewhere — which rustup puts
on your `PATH`. Run it again to update; `cargo uninstall devpage` removes it.

</Tab>
<Tab title="From a checkout">

From the root of a devpage checkout:

```sh
cargo install --path . --locked     # onto your PATH
cargo build --release               # or just build: target/release/devpage
```

Without installing, `cargo run -- <path>` runs it from the checkout. In a
checkout, rustup also adds the `wasm32-unknown-unknown` target that
`rust-toolchain.toml` names; only rebuilding the vendored design engine uses
it ([Developing devpage](boxes/engineering/developing-devpage/README.md#the-vendored-design-engine)).

</Tab>
</Tabs>

## Run it

Give it a document file, a folder holding a `devpage.yml`, a single
Markdown file, or a `.design` file; with no argument it uses `devpage.yml` in the current folder.
devpage prints one line — the document's title and the address it serves on —
then logs each request. For these docs, whose title is `devpage`, run from the
top of the devpage repository:

<Terminal>

```text
$ devpage docs
devpage → http://localhost:3030
```

</Terminal>

Open that address in a browser; stop devpage with <Keys>Ctrl C</Keys>. `devpage --help`
lists the commands — `check`, `pdf` and `video` beside serving — and the flags; the
exact rules are in the
[configuration reference](boxes/viewer/configuration-reference/README.md#command-line).

To serve the docs of another repository, give it a document file — see
[Documenting another repository](boxes/viewer/writing-a-document/README.md#documenting-another-repository).
To serve docs to a team, see [Deploying devpage](boxes/viewer/deploying/README.md).

## Licence

devpage is released under the [MIT licence](../LICENSE). Third-party code built
into the binary keeps its own licence: the
[crates devpage depends on](boxes/engineering/dependencies/README.md) and the
[vendored pele bundle](boxes/viewer/vendored-pele/README.md), whose MIT licence
ships beside it.
