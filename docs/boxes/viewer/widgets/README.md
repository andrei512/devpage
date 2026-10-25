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
  prop, for widgets that show or read their source.
- **Self-closing.** A widget without content can close itself: `<Keys>⌘ P</Keys>`
  has content, `<KindCards />` has none.

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
| `Example` | `title` (optional) | Markdown | The Markdown source, highlighted, beside — or, on a narrow page, above — what it renders to. |
| `FileTree` | — | A Markdown list, nested for folders | A folder tree with file and folder icons. A list item's link makes its entry a link; text after the name is shown as a note beside it. |
| `Terminal` | `title` (optional) | Text in a code block | A terminal window. Lines starting with `$ ` are commands, shown with a prompt; the rest is output. |
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

Every `.tsx` file in the folder is a widget, named by its file name:
`KindCards.tsx` is `<KindCards>`. The field works the same for every kind; a
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
  (`import { format } from "./format"`). Files a widget imports are compiled
  with it; a helper file that isn't a widget names no tag. npm packages can't be
  imported — devpage has no package manager.
- **Images** are referenced by their address from the top of the served folder
  (`/docs/images/logo.svg`), since a widget appears on many pages.
- **One name, one widget.** A project widget can't take a system widget's name:
  `Callout.tsx` in a project is a problem, so system widgets that use `Callout`
  keep getting the one they were written for
  ([the rule](../constraints/one-name-one-widget.md)).

devpage compiles each widget from TSX when a page needs it, and again when its
file changes — saving a widget updates every open page that uses it, like
saving a page does. There is no build step.

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
- a project widget with a system widget's name, or a file name that doesn't
  start with a capital letter;
- a widget file that doesn't compile, with the compiler's message and line;
- a widget, or one of its prop expressions, that throws while rendering — shown
  in the widget's place, with the error, in the browser.

## Under the hood

When devpage renders a page, it finds the tags of known widgets in the
Markdown, parses their props, and renders their content as Markdown. Each
widget becomes a placeholder element carrying its name, its props' source and
its rendered content. In the browser, the page script loads React — vendored
into devpage, like pele — and the page's widgets, compiled from TSX by `oxc`
and served under `/__devpage/widgets/`, then mounts each placeholder: on page
load, and again after every live update. The system widgets are `.tsx` files in
devpage's own `assets/widgets/`, compiled the same way and built into the
binary.
