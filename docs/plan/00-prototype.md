# Prototype report — design/widgets

Measured: started 20:17:02. The first end-to-end path — `<Callout kind="warning">`
with Markdown content and a project widget `<KindCards />` composing `Callout`,
compiled by oxc, mounted by vendored React in headless Chrome — ran at 20:23:25,
about 6½ minutes in. Stopped at 20:28 with every part of the design running
once. The design update is not too big.

## What I touched

- **`src/widgets.rs`** (new): the widget set (system widgets from
  `assets/widgets/*.tsx`, a project's from its `widgets` folder, clashes
  reported); `expand` — widget tags in Markdown become placeholders, skipping
  fenced and inline code, props parsed into a JavaScript object literal
  (strings, `{expr}`, bare), content left for comrak; `compile` — TSX to a
  browser module with oxc 0.153.0 (automatic runtime, `react` import source),
  with shims for other widgets the file renders by name.
- **`src/document.rs`**: a `widgets` field on all three kinds.
- **`src/render.rs`**: pages, docs and decks expand widgets and carry an import
  map plus the document's widget list in `<head>`.
- **`src/server.rs`**: `/__devpage/react.js`, `/__devpage/system-widgets/…`,
  `/__devpage/widgets/<served path>` (compiled per request; a module that
  doesn't compile throws the compiler's message). A site page printed alone
  keeps its site's widgets.
- **`src/check.rs`**: unknown tags, name clashes and compile errors are
  problems in the preview and in `devpage check`.
- **`assets/devpage.js`**: mounting — every placeholder, and every
  `pre[lang="mermaid"]` as a `Diagram`, rendered into React elements (nested
  widgets as elements whose props a parent can read), an error boundary per
  outermost widget, exported CSS added once; `data-ready` waits for every
  widget to mount; a live update takes the fresh page's widget list, whose
  project widgets carry their file's change time.
- **`assets/widgets/`**: Callout, Diagram, Example, FileTree, Keys, Tab, Tabs,
  Terminal. **`assets/vendor/react.js`**: React and react-dom 19.3.0 bundled
  into one ESM file with bun (221 KB minified), MIT licence beside it.
- **`docs/widgets/KindCards.tsx`** and `<KindCards />` on the landing page, the
  docs' `devpage.yml` naming the folder.

Tests: 6 unit tests for expanding and compiling, and one Chrome test (ignored,
passes) — mount on load, a live update re-mounts, an edited widget is fetched
again. By hand in headless Chrome: all eight system widgets render, a render
throw shows in the widget's place, the system `Callout` survives a project
`Callout.tsx`, the PDF of a widget page contains the widgets' text, and
devpage's own docs render with no widget errors. Gates green; the 8 Chrome
tests pass; `devpage check docs` exits 0.

## Where the difficulty actually was

- **A newline in a placeholder's `data-source`** ends comrak's HTML block, so a
  widget whose content had blank lines lost its children. Attributes are kept
  on one line (`&#10;`).
- **Nested widgets wrapped in error boundaries** hid their props from their
  parent — `Tabs` couldn't read its `Tab`s' titles. Only the outermost widget
  gets a boundary.
- **Shimming every unresolved capitalized name** turned `DOMParser` into a
  widget lookup. Only names of known widgets are shimmed.
- **The browser's module cache** kept an edited widget; its address now carries
  the file's change time.
- oxc and React worked as documented; nothing else fought back.

## What the design got wrong or left open

- **Rust version.** oxc 0.153.0 (the newest at least two weeks old) declares
  `rust-version = 1.97`; installing says 1.89 or later.
- **React's size** is 221 KB minified as one bundle, not the 140 KB discussed.
- **Lowercase files.** The widgets page calls a widget file that doesn't start
  with a capital letter a problem, but also allows helper `.tsx` files in the
  folder, which are lowercase. The prototype treats lowercase `.tsx` as
  helpers, not problems.
- **`FileTree` needs the content's structure.** Its content arrives as React
  elements (`children`) and Markdown text (`source`); walking a nested list is
  easiest from the rendered HTML, so the prototype also passes an `html` prop.
  The design names only `children` and `source`.
- **A site page on its own.** The design says a doc's widgets are its own; a
  site page printed as a one-chapter doc has none, so it would lose the site's
  widgets. The prototype gives it its site's.
- **`watch` narrows widgets too.** A site whose `watch` names only Markdown
  doesn't refresh when a widget file changes; devpage's own docs needed
  `widgets/*.tsx` in their list. The design doesn't say whether the widgets
  folder is always watched, like document files.
- **Helpers imported by a widget** are still taken from the module cache after
  an edit; only widget files carry a version.

## What is left to be production-quality

- Load only the widgets a page uses, plus what they render by name (today every
  widget of the document is loaded on every page); cache compiled modules by
  change time instead of compiling per request.
- Version helper modules too, so an edited helper reaches open pages.
- Tests: route tests for the widget endpoints; check tests for each widget
  problem; Chrome tests for Tabs, Example, FileTree, Terminal, Keys, a deck and
  a doc with widgets, comments anchored on text inside a widget, and a PDF
  with widgets.
- The docs adoption the operator asked for — Example, FileTree, Tabs,
  Terminal, Callout and Keys across the pages listed in the design
  conversation; only KindCards is in place.
- Docs: a vendored-React page like the pele one (version, how it was bundled,
  licence), the dependencies page (oxc), developing-devpage (the widgets
  module and assets), installing's Rust version, and the widget styles in the
  stylesheet described where themes are.
