# Unreleased

On `main` after 0.1.0, or on its way there; published with the next release.

<Callout kind="warning" title="Breaking: Rust 1.97 or later">
Widgets are compiled by `oxc`, which needs Rust 1.97. Building devpage with an
older toolchain fails; run `rustup update` first.
</Callout>

<Callout kind="warning" title="Breaking: capitalized tags are widgets">
A tag in Markdown whose name starts with a capital letter is a widget. One that
names no widget — `Option<String>` written in prose, say — is now a problem and
shows an error in its place. Put such text in backticks.
</Callout>

## Widgets

React components written as HTML tags in Markdown, with props in React syntax
and Markdown content ([Widgets](../boxes/viewer/widgets/README.md)).

- **System widgets** ship with devpage: `Callout`, `Tabs` and `Tab`, `Example`,
  `FileTree`, `Terminal`, `Keys`, and `Diagram`, which every `mermaid` fence
  now renders through.
- **Project widgets**: capitalized `.tsx` files in the folder a document file's
  `widgets` field names, compiled from TSX with no build step.
- React 19.3.0 is vendored into devpage.

## The navbar, the sidebar and a theme switch

- **`navbar`** on a site: links, one-level dropdowns and Lucide icons across
  the top ([The navbar](../boxes/viewer/organizing-navigation/README.md#the-navbar)).
- **`sidebar: false`** hides a site's Topics sidebar.
- **A light/dark switch** in every page's top-right corner, following the
  system until clicked; `theme-selector: false` turns it off.
- **`logo`** on any kind: an image before the title and the browser tab's icon.

## Smaller changes

- The paragraph comment button is a speech-bubble icon to the right of the
  block, kept inside narrow windows.
- Diagrams have no frame; tables scroll inside their box on narrow screens.
- devpage's own docs use widgets, and ship three examples, one per kind: the
  docs themselves, these release notes, and [the devpage talk](../talks/devpage/devpage.yml).

## Upgrading

<Terminal>

```
$ rustup update
$ cargo install devpage --locked
```

</Terminal>
