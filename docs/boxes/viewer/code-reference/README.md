# Code reference

A site's code reference is a page for every module, type and function of the
code it names, in the style of DocC: made from the code itself, live as you
save, checked when prose links to it, and never written to disk. It is a task
article under the [viewer](../README.md). devpage's own is at
[the Rust reference](../../../reference/rust).

## Enabling it

A site enables the system extension `code`, one namespace per language, with
the files to read as glob patterns relative to its document file:

```yaml
extensions:
  code:
    config:
      rust:
        files:
          - "../core/src/**/*.rs"
          - "../src/**/*.rs"
        private: true
```

| Field | Meaning |
| --- | --- |
| `files` | Glob patterns of the namespace's files, inside the served folder. `.gitignore`d and hidden files are left out. |
| `language` | The language they are written in; the namespace's name when left out. devpage knows `rust`. |
| `query` | A tree-sitter query file replacing the language's built-in one — see [Changing what is read](#changing-what-is-read). |
| `private` | Whether items no `pub` path reaches get pages too. Default: `false`, the public API only. |

`code` takes no `package`: it is built into devpage. Only a site has a code
reference; `code` on a doc or a deck stops devpage with
`extensions: code: only a site has a code reference`.

## The pages

The pages are under `reference/<namespace>/` in the site's folder, shown in the
site's frame like its own pages:

- `reference/rust` lists the crates.
- `reference/rust/devpage_core` is a crate, `reference/rust/devpage_core/answer`
  a module: its doc comment and its items, grouped by kind, each with the first
  sentence of its doc.
- `reference/rust/devpage_core/answer/Answers` is an item: where it is declared,
  its declaration, its doc comment, and a section for each field, variant and
  method — `#new`, `#path` — and the traits it implements.

A doc comment reads as the page's Markdown: its headings go below the page's
own, an unlabelled code block is Rust with rustdoc's hidden `# ` lines left
out, and rustdoc's links to names show as the name. An item a `pub use` makes
public in another module has its page there; its address where it is declared
leads to the same page.

Nothing is on disk at these addresses. A real file there wins, as anywhere in
the served folder.

## Linking to code

Link to a page or a section like to any page of the site:

```markdown
[`Answers::path`](../reference/rust/devpage_core/answer/Answers#path)
```

[`devpage check`](../checking-links/README.md) and the preview check it: a
name the code no longer has is `broken link … — no such code; did you mean …?`,
with the closest page, and a member the item doesn't have is
`broken link … — no section #…`. A rename in the code fails the check of every
page that still links to the old name. A Topics entry can list a page of the
reference: it opens like a document, with no children in the sidebar.

## Adding prose

A Markdown file anywhere in the site whose first line is the target's id adds
what follows to that target's page, after its doc comment:

```markdown
# ``rust:devpage_core::answer::Answers``

Every route is decided here, so it is tested without a socket.
```

## Changing what is read

devpage parses each file with [tree-sitter](https://tree-sitter.github.io/)
and the language's built-in query, [`rust.scm`](../../../../core/src/code/rust.scm).
A query says what a declaration is with captures: `@item.<kind>` with `@name`
and an optional `@visibility`; `@attach` with `@attach.to` and
`@attach.trait` for a block whose members belong to a type, an `impl`; and
`@reexport` with `@reexport.path` for `pub use`. Point `query` at your own file
to read the code differently. A query that doesn't compile stops devpage,
naming its line; a change to it reads every file again.

What a query can't see, a Rust linker resolves across files: the crate from
the nearest `Cargo.toml` (its `[lib] name`, or its `[package] name` with `-`
as `_`), the module from the file's place under `src/`, the type an `impl` in
another file belongs to, what `pub use` makes reachable, and whether a `pub`
path reaches an item from its crate's root.

## Live, and in memory

devpage parses every file when it loads the site, across the machine's cores,
and holds what they declare in memory. A save parses that one file again and
links the namespace again; open pages refresh. A namespace's folders are
watched whatever the site's `watch` list says. Nothing is written to disk and
nothing has to run first: devpage's own 68,000 lines of Rust add no time a
`devpage check` can measure.

## What it doesn't see

It reads the code as written, not as compiled: no macro output, no `#[cfg]`
evaluation, no `#[path]` attributes, no inferred types and no links a doc
comment writes to a Rust path. A file outside its crate's `src/` — a test, a
bench, `build.rs` — is no module. Search doesn't index the reference's pages,
and a code reference isn't published by `devpage publish`.
