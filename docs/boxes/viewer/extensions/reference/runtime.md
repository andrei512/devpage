# Local runtime protocol

The local adapter loads a separately built Rust library through the [native boundary](README.md#rust-sdk-and-native-boundary). The extension API crate exports that boundary with `export_extension!(Type)`. The type implements `Extension::invoke`; requests and replies are JSON values in memory.

## Activation and inputs

The host validates `extension.yml`, then calls `create` with `instance`, `id`, and `config`. The reply declares:

- `input`: a recursive source directory, relative to the document configuration.
- `input_extensions`: the text-file suffixes to capture.
- `file_extensions`: source suffixes whose viewer and target index the package supplies.
- `grammar`: an optional bundle-relative Syntect syntax file.
- `modules`: package-relative module names mapped to TS, TSX, or JavaScript source.
- `widgets`: widget names mapped to their default-exporting module names.
- `styles`: package-relative stylesheet names mapped to scoped CSS source.

Captured files stay inside the served folder. Symlinks are rejected. Hidden directories are skipped. The host supplies a sorted map of project-relative paths to UTF-8 contents in `prepare.sources`. Preparation returns a nonempty `revision`. Inputs join devpage's watch set even when the site's ordinary watch patterns would exclude them.

The host prepares changed inputs before sending its existing refresh event. A failed preparation becomes a visible error; a later successful capture recovers. Package-code changes require a restart.

## Rendering and navigation

Subsequent requests include `instance`, `revision`, and the instance's mounted `base` URL.

- `augment` receives a page `path` and Markdown `source`, and returns `source`. Added content enters normal highlighting, heading, link, checking, and search processing.
- `embed` receives `page`, `target`, `label`, and a page-local numeric `identity`, and returns `html`. A source-format image reference asks for this presentation.
- `view` receives an existing source `path`, and returns `layout: workspace`, `title`, and `content`. Devpage supplies the page frame; content can contain ordinary registered widget placeholders. A file view does not return a separate HTML application.
- `targets` receives a source `path`, and returns valid fragment `ids`.
- `route` receives the URL suffix beneath the package namespace, and returns text `body`, `mime`, and HTTP `status`.

A content render pins its extension bindings to prepared revisions. Providers must honor those revisions or return an explicit error. Namespaced responses use `no-store`; source-view paths retain the normal served-folder checks. Publishing a document with active native extensions fails explicitly.

## React modules and workspace pages

Package modules use the same TSX compiler and JSX runtime as project widgets. Relative imports resolve only to declared package modules; missing imports fail activation. The module and stylesheet URLs include a digest of the package sources and host build. Widget names join the document’s widget set. Devpage supplies its React import map, mounting, unmounting, error handling, and live refresh lifecycle.

A workspace uses the normal page script and theme infrastructure. Its content fills the viewport, with the tool’s own toolbar, instead of inheriting article navigation, width, or padding. The tool owns its canvas and inspector within that area. Ordinary article pages retain their existing layout; package CSS must be scoped to its own elements.

Props supplied by devpage, including `source`, `html`, and `children`, keep their existing widget meaning. Domain inputs should use distinct names, such as `modelSource`.

## Scope of this adapter

This is the local runtime slice of the [full contract](README.md). It supports one recursive input directory per instance, synchronous content contributions, static SVG embeds for print, and text route responses. It does not yet implement general anchored contribution provenance, arbitrary input queries, lifecycle disposal, or asynchronous readiness declarations. Those remain design work; the runtime does not claim them.

Egraph’s shared React SVG module renders both the interactive canvas and server-rendered embeds. The standalone shell mounts the same React viewer that devpage mounts as a widget. It keeps at most eight prepared revisions in memory; requesting an unavailable revision fails rather than returning another revision.
