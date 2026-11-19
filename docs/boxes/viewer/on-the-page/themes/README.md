---
page-type: task
---
# Themes

How the light and dark themes color everything — the page, highlighted code,
diagrams and widgets — from one set of tokens, how a reader switches between
them, and how far a page can change the look. It sits under [On the page](../README.md); every token is on
[Theme tokens](tokens/README.md).

## Choosing a theme

Every page — a site's, a doc, a deck — has a theme switch in its top-right
corner: one small icon button that switches between light and dark. It shows
the theme a click switches to — a moon in light, a sun in dark. Until the reader
clicks it, the page follows their system setting; after, the browser remembers
the theme they switched to, for the address it was made on. A switch changes
everything drawn from the tokens at once: the page, highlighted code, diagrams
and widgets — and every design on the page, in its own theme of the same
name. An embedded design that uses its themes has a switch of its own on its
backdrop, which turns that embed alone and isn't remembered. On a site with a navbar the button sits at the right end of the
bar. It is never printed, and a deck hides it in fullscreen. A document file
turns it off with `theme-selector: false`; the page then follows the system
setting, as before the switch existed.

## Changing the look

<Callout kind="note" title="No theme setting">
The themes can't be changed per document. A document file has no theme
field — any field its kind doesn't have stops devpage with an error — and the
stylesheet is compiled into the binary, so there is no custom stylesheet option
and no way to add a third theme without changing `core/assets/style.css` and
rebuilding devpage.
</Callout>

What a page can do is carry its own CSS. Raw HTML passes through, so a
`<style>` block in a page's Markdown is part of that page:

```markdown
<style>
:root { --accent: #c00000; }
</style>
```

It applies while that page is shown — to the whole frame, the sidebar
included, since a `<style>` element isn't scoped to the article — and to no
other page: opening another page, by a link or a fresh load, replaces the
page content and the `<style>` with it. A token set this way, as above,
recolors everything that uses it, in both themes, because the rule comes after
the built-in stylesheet; scope it to `:root[data-theme="dark"]` (or `"light"`)
to change one theme only — a `@media (prefers-color-scheme: …)` rule would
follow the system, not the reader's choice. Nothing checks such CSS, and it
rides along with the page's content, so use it for one page's needs rather
than as a theme.

## Topics

- [How themes reach code and diagrams](how-themes-reach-code-and-diagrams/README.md)
  — the `hl-` classes highlighted code carries, and the pele variables diagrams
  draw with.
- [Theme tokens](tokens/README.md) — every token of the light and dark themes
  and what it colors.
