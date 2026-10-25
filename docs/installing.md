# Installing devpage

How to get a `devpage` program you can run on any document: installing it from
crates.io, what `devpage pdf` needs, building it from a checkout instead, and
starting it. It is a task page under the [devpage landing page](README.md);
[Getting started](getting-started.md) then walks a first site, doc and deck.

## What you need

- **Rust 1.97 or later**, with `cargo` — devpage is installed by building it.
  The locked `oxc 0.153.0`, which compiles widgets, declares
  `rust-version = 1.97`, the highest of any dependency. Check what you have with `cargo --version`; install or update
  Rust with [rustup](https://rustup.rs) (`rustup update`).
- **Chrome or Chromium**, only for `devpage pdf`. Without one installed,
  `devpage pdf` downloads a Chromium once and keeps it in devpage's cache
  folder ([Printing to PDF](boxes/viewer/pdf/README.md#the-chrome-it-drives)).

Nothing else is needed: the stylesheet, the page script and the diagram
renderer are compiled into the binary, and diagrams are drawn by the reader's
browser.

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

Without installing, `cargo run -- <path>` runs it from the checkout.

</Tab>
</Tabs>

## Run it

```sh
devpage <path>
```

Give it a document file, a folder holding a `devpage.yml`, or a single
Markdown file; with no argument it uses `devpage.yml` in the current folder.
devpage prints one line — the document's title and the address it serves on —
then logs each request. For these docs, whose title is `devpage`:

<Terminal>

```text
$ devpage docs
devpage → http://localhost:3030
```

</Terminal>

Open that address in a browser; stop devpage with <Keys>Ctrl C</Keys>. `devpage --help`
lists the commands — `check` and `pdf` beside serving — and the flags; the
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
