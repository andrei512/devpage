# Test an extension

How to exercise [extensions](../README.md): the tests in this repository, then a package built and run with the normal `devpage` command. No package ships with devpage; the second part builds a small one.

## Run the repository's tests

```sh
cargo test -p devpage-core extensions
cargo test -p devpage-extension-api
```

The core test binds an in-process toy package (no native library) to a site and checks capture, highlighting, page additions, an embed, checked targets, the watch, a failed preparation and its recovery. The browser test compiles package modules and rejects missing or escaping imports. The SDK test calls an exported table directly and checks replies, errors and a caught panic.

## Build and run a package

1. Create a library crate with `crate-type = ["cdylib"]` that depends on `devpage-extension-api` by path, implement `Extension` on a `Default` type, and end with `export_extension!(Type)`. Its `invoke` answers each `op` in the [reference](../reference/README.md#requests).
2. Build it with `cargo build --release` and copy the library (`libtoy.dylib` on macOS) into a bundle folder beside an `extension.yml` naming it, with `target` set to the triple `rustc -vV` prints as `host`.
3. Enable the bundle in a site's `devpage.yml`, as in [Enable a package](../README.md#enable-a-package), with the input folder inside the site.
4. Run `devpage check .`: a link to an owned file with a fragment the package's `targets` omits is reported as broken.
5. Run `devpage .` and open a page: added Markdown, highlighted fences and embeds appear, and the owned file opens as a workspace page.
6. Add, edit and remove a file under the input: open pages refresh with the new capture. Write a non-UTF-8 file there: pages show `Extension preparation failed`, and the owned file's view answers 500 until the file is removed.
7. Run `devpage pdf README.md -o /tmp/toy.pdf`: the printed page keeps its site's extension.

Rebuilding the library needs a devpage restart.
