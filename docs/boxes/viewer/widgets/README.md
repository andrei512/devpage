# Widgets

How to make a page more visual with **widgets** — React components written as
HTML tags in Markdown: the system widgets devpage ships, writing a project's
own, how props and content reach a widget, how widgets are styled, and what
happens when one fails. It is a task and reference article under the
[viewer](../README.md).

```markdown
<Callout kind="warning" title="Reachable only through the proxy">
devpage **believes** the identity header, so expose the proxy, not devpage.
</Callout>

<Tabs>
<Tab title="crates.io">

    cargo install devpage --locked

</Tab>
<Tab title="From a checkout">

    cargo install --path . --locked

</Tab>
</Tabs>
```

## Using a widget

A widget is used like an HTML tag whose name starts with a capital letter —
`<Callout>`, `<FileTree>` — anywhere in a page of any kind: a site's page, a
doc's chapter or a slide.

- **Props.** A quoted value is a string: `kind="warning"`. A value in braces is
  a JavaScript expression, evaluated in the browser: `values={[3, 5, 8]}`,
  `open={false}`, `start={new Date(2026, 0, 1)}`. An attribute with no value is
  `true`: `<Tabs vertical>`.
- **Content.** What sits between the tags is Markdown, rendered by devpage —
  with or without blank lines around it — and arrives as `children`. Links and
  images in it are checked like any other ([Checking links](../checking-links/README.md)).
  The Markdown text itself, exactly as written, also arrives as the `source`
  prop, and the rendered HTML as a string as the `html` prop, for widgets that
  show their source or read the structure of their content.
- **Self-closing.** A widget without content can close itself: `<Keys>⌘ P</Keys>`
  has content, `<KindCards />` has none.
- **Block or inline.** A widget that has its lines to itself — nothing before
  its open tag, nothing after its close tag — is a block. Anywhere else it is
  part of the paragraph around it, at the start of a line included:
  `<Keys>⌘ P</Keys> prints the **doc**.` stays one paragraph, bold and all.

Tag names are matched exactly, and only names of known widgets are widgets: a
capitalized tag that names no widget is a [problem](#when-a-widget-fails), and
a lowercase tag is ordinary HTML.

## System widgets

devpage ships these, in every document:

| Widget | Props | Content | Shows |
| --- | --- | --- | --- |
| `Callout` | `kind`: `note` (default), `tip`, `warning` or `danger`; `title` (optional) | Markdown | A highlighted box with a colored edge, an icon for its kind and an optional title. |
| `Tabs` | — | `Tab` widgets | Tabs, the first one open; clicking a tab's title shows it. Printed, only the open tab shows. |
| `Tab` | `title` | Markdown | One tab of a `Tabs`. Outside a `Tabs` it is a problem. |
| `Example` | `title` (optional) | Markdown | The Markdown source, highlighted, beside — or, on a narrow page, above — what it renders to. A long source line scrolls inside its box; on paper it wraps. |
| `FileTree` | — | A Markdown list, nested for folders | A folder tree with file and folder icons. A list item's link makes its entry a link; text after the name and ` — ` is shown as a note beside it, with its inline Markdown — code, emphasis, links — kept. |
| `Terminal` | `title` (optional) | Text in a code block | A terminal window. Lines starting with `$ ` are commands, shown with a prompt; the rest is output. A long line scrolls on screen and wraps on paper. |
| `Keys` | — | Key names separated by spaces | Each key as a keycap, inline in the text: `<Keys>⌘ P</Keys>`. |
| `Diagram` | — | Mermaid text | The diagram, drawn by pele. Every `mermaid` fence is rendered through it; the tag itself is rarely written. |

A `FileTree`, written as a list so it reads well anywhere:

```markdown
<FileTree>

- garden/
  - [devpage.yml](../configuration-reference/README.md#site) — the site
  - README.md — the root page
  - planting.md

</FileTree>
```

## A project's own widgets

A project adds widgets in one folder, named by its document file's `widgets`
field, relative to that file:

```yaml
kind: site
title: devpage
root: README.md
widgets: widgets          # every .tsx in docs/widgets/ is a widget
```

Every `.tsx` file in the folder whose name starts with a capital letter is a
widget, named by its file name: `KindCards.tsx` is `<KindCards>`. Files whose
names start with a lowercase letter — `format.ts`, `cards.tsx` — are helpers
that widgets import; they name no tag. The field works the same for every kind; a
doc's or deck's widgets are its own.

A widget is a React component — its default export — written in TSX:

```tsx
// docs/widgets/KindCards.tsx
export default function KindCards() {
  const kinds = [
    { name: "Site", href: "/docs/boxes/viewer/organizing-navigation/README.md" },
    { name: "Doc", href: "/docs/boxes/viewer/writing-a-doc/README.md" },
    { name: "Slides", href: "/docs/boxes/viewer/making-slides/README.md" },
  ];
  return (
    <div className="cards">
      {kinds.map((k) => <a className="card" href={k.href}>{k.name}</a>)}
    </div>
  );
}
```

- **It is just React.** Hooks, state, effects and events all work; devpage ships
  React and runs each widget with it.
- **Widgets compose.** A widget can render any other widget — a system widget or
  one of the project's — by name, without importing it.
- **Helpers.** A widget can import `.ts` and `.tsx` files from the same folder
  (`import { format } from "./format"`); they are compiled with it. npm packages can't be
  imported — devpage has no package manager.
- **Images** are referenced by their address from the top of the served folder
  (`/docs/images/logo.svg`), since a widget appears on many pages.
- **One name, one widget.** A project widget can't take a system widget's name:
  `Callout.tsx` in a project is a problem, so system widgets that use `Callout`
  keep getting the one they were written for
  ([the rule](../constraints/one-name-one-widget.md)).

devpage compiles each widget from TSX when a page needs it, and again when its
file changes — saving a widget or a helper updates every open page that uses
it, like saving a page does, whatever a site's `watch` list says. There is no
build step. A page of a site has that site's widgets wherever it is shown,
printed on its own included.

## Styling

A widget is styled the way any React component is, in any mix:

- **The theme's classes** — `className="cards"` — and its tokens.
- **Inline styles** — `style={{ borderColor: "var(--accent)" }}`.
- **Its own CSS**, optionally, exported from the same file as a string:

  ```tsx
  export const css = `
    .kind-cards { display: grid; grid-template-columns: repeat(3, 1fr); gap: 1rem; }
    .kind-cards a:hover { border-color: var(--accent); }
  `;
  ```

  devpage adds it to the page once.

Written with the theme's tokens — `var(--accent)`, `var(--fg)`, `var(--border)`
— a widget follows the light and dark themes with nothing else to do; where it
must differ beyond the tokens, `@media (prefers-color-scheme: dark)` works as
anywhere ([Diagrams and themes](../diagrams-and-themes/README.md#the-themes)).

## On paper

A PDF, and the browser's print, show each widget's **first render** — what it
shows before anyone clicks: a `Tabs` prints its first tab. A widget meant for
paper shows what matters without interaction
([A PDF is the page the browser shows](../constraints/a-pdf-is-the-page.md)).
`devpage pdf` waits for every widget to mount before printing.

## When a widget fails

These are [problems](../checking-links/README.md) — in the page's problems
bar, on the terminal, and failing `devpage check` — and the rest of the page
renders:

- a capitalized tag that names no widget;
- a project widget with a system widget's name;
- a widget file that doesn't compile, with the compiler's message and line;
- a widget, or one of its prop expressions, that throws while rendering — shown
  in the widget's place, with the error, in the browser.

## Under the hood

**On the server** (`src/widgets.rs`), rendering a page finds the tags of known
widgets in its Markdown — skipping fenced and inline code — and turns each into
a placeholder: a `div` for a block widget, a `span` for an inline one, so an
inline widget never breaks its paragraph. The placeholder carries the widget's
name, its props as a JavaScript object literal (quoted values as strings, `{…}`
as written, bare attributes as `true`) and its Markdown source — and, for
`Example`, that source highlighted as Markdown by the same highlighter fenced
code uses, which reaches it as the `sourceHtml` prop. Its content stays between
the tags for comrak to render as Markdown. The page's `<head>` gets an import map
naming React, and the list of the document's widgets with the address of each.

**Compiling.** A widget or helper is compiled from TSX by `oxc` when the
browser asks for it, with React's automatic JSX runtime. Capitalized names a
widget uses without declaring them are other widgets: the compiled module looks
each up by name when it renders, and lists them in `__devpageUses`. A system
widget composes only system widgets; a project widget composes the system
widgets and those of the document whose widgets folder holds it. Every project
widget's address carries its folder's **version** — the latest change time of
the folder and its `.ts` and `.tsx` files — and so does every relative import
inside it, so an edited widget or helper is fetched afresh rather than taken
from the browser's module cache. Compiled modules are remembered, keyed by the
file, its source, the names it can use and the version, so a widget compiles
once per change, not once per request. A file that doesn't compile is served as
a module that throws the compiler's message.

| Address | What it serves |
| --- | --- |
| `/__devpage/react.js` | React and react-dom, [vendored](../vendored-react/README.md), for the import map's `react`, `react/jsx-runtime` and `react-dom/client` |
| `/__devpage/system-widgets/<Name>.tsx` | A system widget, compiled from `assets/widgets/`, built into the binary |
| `/__devpage/widgets/<path>?v=<version>` | A project widget or helper at that path in the served folder, compiled; an import without an extension finds the `.tsx` or `.ts` |

**In the browser** (`assets/devpage.js`), each `pre[lang="mermaid"]` first
becomes a `Diagram` placeholder. Then the page script loads only the widgets the
content names, nested ones included, and those each lists in `__devpageUses`;
adds a widget's exported `css` to the page once; and mounts each outermost
placeholder with React. Nested widgets become React elements their parent can
read — a `Tabs` reads its `Tab`s' titles — and the outermost widget is wrapped
in an error boundary that shows a failure in its place. Each widget gets its
props, `children`, `source` and `html`. Mounting happens on page load and again
after every live update, which takes the fresh page's widget list, so new
versions are fetched. The page marks itself ready only once every widget has
mounted, which is what `devpage pdf` waits for.

Widget and helper files refresh open pages whatever a site's `watch` list says
([Live updates](../live-updates/README.md#the-watcher)).
