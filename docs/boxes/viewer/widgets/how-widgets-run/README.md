# How widgets run

How a widget tag in Markdown becomes a mounted React component: the
placeholder the server writes, how widgets are compiled and served, and how the
page script mounts them. It is a mechanism page under [Widgets](../README.md),
for someone about to change `src/widgets.rs` or the widget code in
`assets/devpage.js`.

```mermaid
flowchart LR
  tag["Tag in Markdown"] --> placeholder["Placeholder<br/>on the server"]
  placeholder --> fetch["Module fetched<br/>compiled by oxc"]
  fetch --> mount["Mounted<br/>with React"]
  click placeholder href "#on-the-server" "On the server"
  click fetch href "#compiling" "Compiling"
  click mount href "#in-the-browser" "In the browser"
```

## On the server

In `src/widgets.rs`, rendering a page finds the tags of known
widgets in its Markdown — skipping fenced, indented and inline code and HTML
comments — and turns each into
a placeholder: a `div` for a block widget, a `span` for an inline one, so an
inline widget never breaks its paragraph. The placeholder carries the widget's
name, its props as a JavaScript object literal (quoted values as strings, `{…}`
as written, bare attributes as `true`) and its Markdown source — and, for
`Example`, that source highlighted as Markdown by the same highlighter fenced
code uses, which reaches it as the `sourceHtml` prop. Its content stays between
the tags for comrak to render as Markdown. The page's `<head>` gets an import map
naming React and pele, the list of the document's widgets with the address of
each, and a `modulepreload` for every module the page's widgets need — their
own, those they render by name, their helpers, React and, with a diagram,
pele ([the load set](../../rendering-a-page/README.md#the-load-set)).

## Compiling

A widget or helper is compiled from TSX by `oxc` when the
browser asks for it, with React's automatic JSX runtime. Capitalized names a
widget uses without declaring them are other widgets: the compiled module looks
each up by name when it renders, and lists them in `__devpageUses`. A system
widget composes only system widgets; a project widget composes the system
widgets and those of the document whose widgets folder holds it.

Every widget's address carries its **version**, `?v=<hash>`: a hash of its
source, the source of every module its relative imports reach — directly or
through other helpers — with each one's path, the widget names it can render,
and devpage's build. That is everything its compiled module is made from, so
the address changes exactly when the module's bytes would: an edited helper
gives new addresses to the widgets that import it, and to no other. Each
relative import inside a compiled module is rewritten to the imported
module's own address — `./label` becomes
`/__devpage/widgets/docs/widgets/label.ts?v=<its hash>` — so a widget and a
page preloading its helper name the same address. A module asked for at its
current version is served `immutable`, and the browser never asks for it
again; at any other version, or none, it is served as it is now, with
`no-cache` ([Caching](../../serving-pages/README.md#caching)). Every address
comes from one function, `address` in `src/addresses.rs`. The files a version
is made from are read once and remembered until their change time or size
moves, or the watcher reports them changed. Compiled modules are remembered
too, keyed by the file, its source, the names it can use and the addresses
its imports are rewritten to, so a widget compiles once per change, not once
per request. A file that doesn't compile is served as
a module that throws the compiler's message, and so is a widget whose relative
imports reach a helper that doesn't compile — with the helper's message — so
the page shows why in the widget's place rather than only that its import
failed.

| Address | What it serves |
| --- | --- |
| `/__devpage/react.<hash>.js` | React and react-dom, [vendored](../../vendored-react/README.md), for the import map's `react`, `react/jsx-runtime` and `react-dom/client` |
| `/__devpage/system-widgets/<Name>.tsx?v=<version>` | A system widget, compiled from `assets/widgets/`, built into the binary |
| `/__devpage/widgets/<path>?v=<version>` | A project widget or helper at that path in the served folder, compiled; a path without an extension finds the `.tsx` or `.ts` |

## In the browser

In `assets/devpage.js`, each `pre[lang="mermaid"]` first
becomes a `Diagram` placeholder. Then the page script loads only the widgets the
content names, nested ones included, and those each lists in `__devpageUses`,
importing them alongside React rather than after it;
adds a widget's exported `css` to the page once; and mounts each outermost
placeholder with React. Nested widgets become React elements their parent can
read — a `Tabs` reads its `Tab`s' titles — and the outermost widget is wrapped
in an error boundary that shows a failure in its place; a widget whose module
didn't load shows the reason it didn't. Each widget gets its
props, `children`, `source` and `html`. Mounting happens on page load and again
after every live update that changed the page, which takes the fresh page's
widget list, so new versions are fetched. The page marks itself ready only once every widget has
mounted, which is what `devpage pdf` waits for.

Widget and helper files refresh open pages whatever a site's `watch` list says
([Live updates](../../live-updates/README.md#the-watcher)).
