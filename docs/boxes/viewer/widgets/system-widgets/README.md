# System widgets

Every widget devpage ships, in every document, each shown live beside the
Markdown that makes it. It is the reference page under [Widgets](../README.md);
how to write props and content is on [Using widgets](../using-widgets/README.md).

| Widget | Props | Content |
| --- | --- | --- |
| [`Callout`](#callout) | `kind`: `note` (default), `tip`, `warning` or `danger`; `title` (optional) | Markdown |
| [`Tabs`](#tabs-and-tab) | — | `Tab` widgets |
| [`Tab`](#tabs-and-tab) | `title` | Markdown |
| [`Example`](#example) | `title` (optional) | Markdown |
| [`FileTree`](#filetree) | — | A Markdown list, nested for folders |
| [`Terminal`](#terminal) | `title` (optional) | Text in a code block |
| [`Keys`](#keys) | — | Key names separated by spaces |
| [`Steps`](#steps) | — | A numbered Markdown list |
| [`Cards`](#cards) | `columns` (optional, 2–4; default fits the width) | A Markdown list of links |

`Notes` is taken too, though it is not a widget: it holds a slide's
[notes](../../making-slides/README.md#speaker-notes) and is never shown. So is
[`DevpageSidebar`](#devpagesidebar), devpage's own chrome. A `mermaid` fence
is no widget: the server draws it ([Diagrams](../../diagrams/README.md)).

## Callout

A highlighted box with a colored edge, an icon for its kind and an optional
title.

<Example>

<Callout kind="tip" title="Save often">
Every open page updates the moment you do.
</Callout>

<Callout kind="danger">
Never expose devpage without the proxy.
</Callout>

</Example>

## Tabs and Tab

Tabs, the first one open; clicking a tab's title shows it. Printed, only the
open tab shows. A `Tab` outside a `Tabs` is a problem.

<Example>

<Tabs>
<Tab title="macOS">

Press <Keys>⌘ P</Keys>.

</Tab>
<Tab title="Linux">

Press <Keys>Ctrl P</Keys>.

</Tab>
</Tabs>

</Example>

## Example

The Markdown source, highlighted, beside — or, on a narrow page, above — what it
renders to. A long source line scrolls inside its box; on paper it wraps. Every
example on this page is one.

## FileTree

A folder tree with file and folder icons, written as a list so it reads well
anywhere. A list item's link makes its entry a link; text after the name and
` — ` is shown as a note beside it, with its inline Markdown — code, emphasis,
links — kept.

<Example>

<FileTree>

- garden/
  - [devpage.yml](../../configuration-reference/README.md#site) — the site
  - README.md — the root page, with `## Topics`
  - planting.md

</FileTree>

</Example>

## Terminal

A terminal window, written as a code block. Lines starting with `$ ` are
commands, shown with a prompt; the rest is output. A long line scrolls on
screen and wraps on paper.

<Example>

<Terminal>

```
$ devpage garden
Garden → http://localhost:3030
```

</Terminal>

</Example>

## Keys

Each key as a keycap, inline in the text.

<Example>

Press <Keys>⌘ P</Keys> to print, <Keys>F</Keys> for fullscreen.

</Example>

## Steps

Numbered steps down the page, written as a numbered list. Each item's first
line is its title and the lines after it its body; a paragraph of lines
starting with `$ ` becomes command chips.

<Example>

<Steps>

1. Install devpage
   It builds from crates.io.

   $ cargo install devpage --locked

2. Run it from the top of your repository
   $ devpage docs

</Steps>

</Example>

## Cards

A grid of link cards, written as a list of links. Each item's first link is the
card's title and target, the text after it its one-line summary; an `(icon)`
before the link adds a [Lucide](https://lucide.dev/icons/) icon. An icon name
Lucide doesn't have is a [problem](../../checking-links/README.md).

<Example>

<Cards>

- (rocket) [Making a site](../../making-a-site/README.md) — pages with a sidebar.
- (file-text) [Writing a doc](../../writing-a-doc/README.md) — one long page.

</Cards>

</Example>

## DevpageSidebar

A site's sidebar: devpage's own chrome, not a widget for pages. devpage
mounts it on every site page with a sidebar — a search box at its top,
then the tree, collapsed, the branch to the page shown open, a chevron on
every entry with children — and keeps it mounted as the reader moves between
pages
([Organizing navigation](../../organizing-navigation/README.md#opening-and-closing),
[the search box](../../search/README.md#the-search-box),
[how it is rendered](../../navigation-tree/README.md#rendering-the-sidebar)).

Its name is reserved like every system widget's: a project's
`DevpageSidebar.tsx` is a problem. The **`Devpage` prefix** marks the widgets
that are devpage's own chrome, so their names stay clear of a project's: a
project may have its own `Sidebar`.
