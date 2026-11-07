# Code reference

A site's [code reference](../glossary/README.md#code-reference) is a page for
every module, type and function of the code it names:
made from the code as it is now, live as you save, checked when prose links
to it, and never written to disk. This page is how to use it; how devpage
reads the code is in [Reading the code](reading-the-code/README.md). It
belongs to the [viewer](../README.md). devpage's own is
[the Rust reference](../../../reference/rust).

```mermaid
flowchart LR
  code["Code files<br/>crates/**/*.rs"] --> read["Read: tree-sitter<br/>and a query"]
  read --> link["Link: crates, modules,<br/>impls, pub use"]
  link --> pages["A page per item<br/>and member"]
  prose["Prose files<br/># ``rust:…``"] --> pages
  pages --> check["devpage check"]
  pages --> search["Search"]
  click read href "./reading-the-code/README.md#reading-one-file" "Reading one file"
  click link href "./reading-the-code/README.md#linking-the-files" "Linking the files"
  click pages href "#the-pages" "The pages"
  click prose href "#adding-prose" "Adding prose"
  click check href "#linking-to-code" "Linking to code"
  click search href "../search/README.md#what-is-searched" "What is searched"
```

## Enabling it

A site enables the [system extension](../glossary/README.md#system-extension)
`code` in its document file, with one
[namespace](../glossary/README.md#namespace) per language and the files to
read:

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

`files` are glob patterns relative to the document file, inside the served
folder. `private: true` gives items no `pub` path reaches pages too; without
it the reference is the source-visible API. Python/PyO3 and TypeScript
namespaces use their own export and module conventions. Every field, and the errors a wrong one
stops devpage with, are in the
[configuration reference](../configuration-reference/README.md#code). Only a
site has a code reference.

## The pages

The pages are under `reference/<namespace>/` in the site's folder, shown in the
site's frame like its own pages, and laid out as Apple lays out a symbol page,
with no label the layout already says:

- The kind — Structure, Method of `Answers`, Module… — above the name, and the
  doc comment's first paragraph as the abstract.
- The declaration, as written without its body, on a card, with the file and
  line it is in under it.
- The rest of the doc comment, and any [prose](#adding-prose) that adds to it.
- What it holds, grouped by kind — a module's modules, structures,
  enumerations, traits, functions…; a type's fields, variants, associated
  functions and methods — each its declaration on one line with its name in
  bold, linked, and its abstract under it. A type's traits close the page.

Every [item](../glossary/README.md#item) has a page, and so does every
[member](../glossary/README.md#member): `reference/rust` lists the crates,
`reference/rust/devpage_core/answer` is a module, `…/answer/Answers` a
structure and `…/answer/Answers/path` its method. A Topics entry for
`reference/rust` opens the reference's tree in the sidebar: its crates, and
under each module its modules and types. A function's or a member's page keeps
its module or its type open there.

Rust doc comments read as the page's Markdown: its headings go below the page's
own, an unlabelled code block is Rust with rustdoc's hidden `# ` lines left
out, and rustdoc's links to names show as the name. An item a `pub use` makes
public in another module has its page there; its address where it is declared
leads to the same page. An item marked `#[cfg(test)]` or `#[doc(hidden)]` has
none, nor does anything inside it.

A real file at one of these addresses wins, as anywhere in the served folder.
An address under `reference/` with nothing there answers the
[404 page](../making-a-site/README.md#a-page-for-missing-addresses), offering
the closest name: `nothing at …/my_lib/Stor — did you mean …/my_lib/Store?`.

## Linking to code

Link to a page like to any page of the site:

```markdown
[`Answers::path`](../reference/rust/devpage_core/answer/Answers/path)
```

[`devpage check`](../checking-links/README.md) and the preview check it: a
name the code no longer has is `broken link … — no such code; did you mean …?`,
with the closest page, and a `#fragment` the page doesn't have is
`broken link … — no section #…`. So a rename in the code fails the check of
every page that still links to the old name.

## Adding prose

A [prose file](../glossary/README.md#prose-file) is a Markdown file anywhere in
the site whose first non-blank line is a heading of an item's or a member's
[id](../glossary/README.md#id), `<namespace>:<path>`, between double backticks.
What follows it is added to that page, after the doc comment:

```markdown
# ``rust:devpage_core::answer::Answers``

Every route is decided here, so it is tested without a socket.
```

A prose file whose id names nothing — the item was renamed or removed — is a
problem: `devpage check` reports `<file>:<line>: ``<id>`` names no code` and
exits `1`, and the reference's pages list it. So is a second prose file for an
id: `` ``<id>`` is added to by <file> already ``, the first in path order being
the one added.

## Changing what is read

devpage reads each file with the language's built-in tree-sitter query,
[`rust.scm`](../../../../core/src/code/rust.scm). To read the code
differently — another kind of declaration, a macro your code declares items
with — copy it, change it, and point the namespace's `query` at your copy.
What a query can capture, and what devpage does with each capture, is in
[Reading one file](reading-the-code/README.md#reading-one-file). A query that
doesn't compile stops devpage at start, naming its line; one saved while devpage
runs is a problem on the reference's pages until it compiles, and the last
query that did keeps reading the code.

## What it doesn't see

It reads source declarations, not compiler output: it does not evaluate arbitrary
macros, `#[cfg]`, `#[path]`, build scripts or inferred types. Selected simple
Rust templates are expanded as described below; strict Rust namespaces refuse
unhandled item-position macros. A Rust file outside its crate's `src/` — a
test, bench or `build.rs` — is not a module. Runtime and compiler inventory
comparisons remain separate checks when they define the published surface.

## Publishing it

A code reference is meant to be read by running `devpage` in the repository,
where it reads the code as it is now. A site that enables it can still be
[published](../server/publishing/README.md): the publish carries each
grammar namespace's files, the `Cargo.toml` naming their crate, its query file and its
prose files, and the server builds the version's reference from them as
`devpage` does locally. Only code inside the published folder can be carried,
as for any file a publish holds. Native semantic providers currently require
serving an immutable source/package snapshot; native-extension publishing is
not supported.

## Language namespaces and semantic providers

A namespace can select `language: python`, `typescript`, `tsx`, or `pyo3` as
well as `rust`. `root` is the source module root relative to the site file;
`module` adds a `::`-separated prefix. Python and TypeScript names use their
file's path from `root`, without its suffix or terminal `__init__`/`index`,
followed by the declaration name. `layout: flat` uses query names directly.
Rust keeps crate/module reachability unless a flat layout is selected.
TypeScript named local exports and relative named reexports resolve to their
source declarations. Union/intersection field branches stay in their whole
type declaration, because they have no single field identity. Python reads
classes/functions and their docstrings; leading-underscore names are private.
PyO3 displays Python-visible callable parameters, including explicit defaults
and keyword-only rules, while links lead to their Rust binding source.

Queries can capture `@doc`, `@signature`, and `@public` alongside
`@item.<kind>` and `@name`. String names and docs are decoded as string literals.
`#set! visibility "public"`, `#set! name "..."`, and `#set! doc "..."`
provide constant query properties. These declarations still use the normal
reference pages, source links, search and prose augmentation.

`strict: true` requires a nonempty catalog and an abstract on every source
item and member. Automatically generated folder containers are navigation,
not source declarations, and do not require abstracts. Source parse and read failures are always check problems. Grammar
reading is source-based, not proof of compiler exports or runtime bindings;
projects should compare the native inventory against their compiled API tests
when runtime registration defines the public surface.

For project-specific semantic joins, a namespace may use a native extension:

```yaml
extensions:
  domain:
    package: ./reference-extension
    config: {input: ../src}
  code:
    config:
      nodes: {provider: domain, strict: true}
```

A provider namespace has no `files` or `query`. The extension's `references`
operation receives `namespace`, `instance`, and its prepared `revision`, and
returns `items` plus optional source-located `problems`. Items use `path`
(an array of names), `kind`, `signature`, `doc`, `file`, `line`, `public`,
`members`, and `implements`; members use `name` instead of `path`. Sources
are relative to the served root and must exist at the given one-based line.
Every parent item must be present. Duplicate identities, invalid sources,
semantic problems and failed preparations fail checking; stale catalogs are
cleared after provider failures.

The extension may declare `code_languages: {rs: rust}` in `create`. Its
`prepare` request then receives `declarations` beside `sources`: each source's
shared grammar result, containing docs, source signatures, attributes,
parent indices, reexports and parse diagnostics. The extension resolves its
own domain relationships and returns declarations; devpage owns rendering,
search, links, supplemental prose and checking for those declarations too.

Selected local Rust macros can be expanded with `macros: [value_type]`.
The supported form is one rule with comma-separated `ident` and `literal`
parameters, substituted into a source template. The normal Rust grammar reads
its generated declarations, including `#[doc = ...]`, tuple fields and impl
methods. Unsupported selected matchers, repetitions, unresolved parameters and
missing local definitions are errors. In strict Rust namespaces, a new
unhandled file-scope invocation is also an error; `ignore_macros` explicitly
lists declaration-position macros outside the reference surface. Expression
macros inside function bodies do not affect this check. This bounded reader does not evaluate
arbitrary macros or build scripts. Source links identify the original invocation.

Native providers request the same expansion with
`code_macros: {rs: [value_type]}`. Each shared grammar result contains
`expansions: [{source, line}]` for semantic parsing of generated fields and
attributes. `devpage_core::code::read_source` exposes the same reader to tests.

`exclude` lists source globs to omit after matching `files`; use it for tests,
fixtures and generated bindings whose documented owner is another namespace.
A TypeScript namespace chooses the TSX grammar for `.tsx` files, while `.ts`
keeps TypeScript parsing. PyO3 reads explicit module registration calls,
class/method/field attributes and exception declarations; runtime API tests
remain the authority for registration coverage.

## Topics

### Mechanisms

- [Reading the code](reading-the-code/README.md) — how one file is read with a
  query, how the files are linked into items, and how the reference stays live
  and searched.

### Reference

- [Language namespaces and semantic providers](#language-namespaces-and-semantic-providers)
  — grammars, strict coverage and native domain declarations.

- [The `code` extension's fields and errors](../configuration-reference/README.md#code)
  — every field of a namespace, and the errors devpage stops with.
- [`devpage_core::code`](../../../reference/rust/devpage_core/code) — the
  implementation, in devpage's own reference.

