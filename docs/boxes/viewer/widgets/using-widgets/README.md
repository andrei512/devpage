# Using widgets

How to put a widget in a page: its props, its content, when it is a block and
when it sits inside a paragraph, and what happens when one fails. It is a task
article under [Widgets](../README.md).

## Props and content

<Example>

<Callout kind="tip" title="Props are attributes">
`kind="tip"` is a string, a value in braces is a JavaScript expression, and the
**Markdown** between the tags is the content.
</Callout>

</Example>

- **Props.** A quoted value is a string: `kind="warning"`. A value in braces is
  a JavaScript expression, evaluated in the browser: `values={[3, 5, 8]}`,
  `open={false}`, `start={new Date(2026, 0, 1)}`. An attribute with no value is
  `true`: `<Chart stacked />` gives a project's `Chart` widget `stacked={true}`.
- **Content.** What sits between the tags is Markdown, rendered by devpage —
  with or without blank lines around it — and arrives as `children`. Links and
  images in it are checked like any other ([Checking links](../../checking-links/README.md)).
  The Markdown text itself, exactly as written, also arrives as the `source`
  prop, and the rendered HTML as a string as the `html` prop, for widgets that
  show their source or read the structure of their content.
- **Self-closing.** A widget without content can close itself: `<Keys>⌘ P</Keys>`
  has content, `<KindCards />` has none.

Tag names are matched exactly, and only names of known widgets are widgets: a
capitalized tag that names no widget is a [problem](#when-a-widget-fails),
shown as an error in its place, never passed through as HTML; a lowercase tag
is ordinary HTML.

## Block or inline

A widget that has its lines to itself — nothing before its open tag, nothing
after its close tag — is a block. Anywhere else it is part of the paragraph
around it, at the start of a line included:

<Example>

<Keys>⌘ P</Keys> prints the **doc**, one paragraph, bold and all.

</Example>

## On paper

A PDF, and the browser's print, show each widget's **first render** — what it
shows before anyone clicks: a `Tabs` prints its first tab. A widget meant for
paper shows what matters without interaction
([A PDF is the page the browser shows](../../constraints/a-pdf-is-the-page.md)).
`devpage pdf` waits for every widget to mount before printing.

## When a widget fails

The rest of the page always renders. These are
[problems](../../checking-links/README.md) — in the page's problems bar, on
the terminal, and failing `devpage check`:

- a capitalized tag that names no widget — also shown as an error in its place;
- a `Tab` outside a `Tabs`;
- a project widget with a system widget's name — the system widget is the one
  that renders;
- a widget or a helper file that doesn't compile, with the compiler's message
  and line — also shown in the place of each widget that is, or imports, it.
  A widgets folder's problems are reported once per document, not per page.

<Callout kind="note" title="Throwing in the browser is not a problem">
A widget, or one of its prop expressions, that **throws while rendering** is
shown in the widget's place, with the error, in the browser. devpage never runs
widget code on the server, so neither the problems bar nor `devpage check` can
see it.
</Callout>
