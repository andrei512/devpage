# Installing devpage

How to get a `devpage` program you can run on any document: the Rust toolchain
it needs, building it from this repository, putting it on your `PATH`, and
starting it. devpage is distributed only as source today — there is no
published binary and no crate — so every install is a build. It is a task page
under the [devpage landing page](README.md); [Getting started](getting-started.md)
then walks a first document end to end.

## What you need

- **Rust 1.89 or later**, with `cargo`. The locked `comrak 0.57.0` declares
  `rust-version = 1.89`, the highest of any dependency (`Cargo.toml` itself
  declares `edition = "2024"`, which needs 1.85). Check what you have with `cargo --version`; the commands on this
  page were checked with `cargo 1.98.1`. The repository pins no toolchain
  (there is no `rust-toolchain.toml`). Install or update Rust with
  [rustup](https://rustup.rs) (`rustup update`).
- **A checkout of the devpage repository.** Every command below runs from its
  root, the folder holding `Cargo.toml`.

Nothing else is needed at run time: the stylesheet, the page script and the
diagram renderer are compiled into the binary (see
[built-in assets](boxes/viewer/serving-pages/README.md#built-in-assets)), and
diagrams are drawn by the reader's browser.

## Build it

```sh
cargo build --release
```

The binary lands at `target/release/devpage`. Run it from there, or copy it
anywhere you like — it is one self-contained file.

## Put it on your PATH

```sh
cargo install --path .
```

This builds the release binary and copies it to cargo's `bin` folder —
`~/.cargo/bin` unless `CARGO_HOME` or `CARGO_INSTALL_ROOT` points elsewhere —
which rustup puts on your `PATH`. Afterwards `devpage` runs from any folder.
Run the same command again after pulling changes; cargo replaces the installed
binary. `cargo uninstall devpage` removes it.

## Run it

```sh
devpage <document.yml | folder>
```

Give it a document's YAML file, or a folder holding a `devpage.yml`; with no
argument it uses `devpage.yml` in the current folder. Relative and absolute
paths both work. devpage prints one line — the document's title and the address it
serves on — then logs each request. For these docs, whose title is `devpage`:

```text
devpage → http://localhost:3030
```

Open that address in a browser. Stop devpage with Ctrl-C. To serve this
repository's own documentation:

```sh
devpage docs              # or, without installing: cargo run -- docs/devpage.yml
```

devpage has no flags — not even `--help`; anything given as the first argument
is read as a path, so `devpage --help` stops with `Error: reading --help`. The
exact rules for the argument, the port and the errors are in the
[configuration reference](boxes/viewer/configuration-reference/README.md#command-line).

To serve the docs of another repository, give that repository a `devpage.yml`
— see [Documenting another repository](boxes/viewer/writing-a-document/README.md#documenting-another-repository).

## Licence

devpage has no licence yet: the repository holds no `LICENSE` file, and
`Cargo.toml` declares none. The [vision](vision.md) describes devpage as open
source; that is an intention, not a grant. Third-party code built into the
binary keeps its own licence: the [crates devpage depends on](boxes/engineering/dependencies/README.md)
and the [vendored pele bundle](boxes/viewer/vendored-pele/README.md), whose MIT
licence ships beside it.
