# Markdown that works

Every Markdown feature a page can use, how code blocks are highlighted, and how
images and other files are served. It is the reference page under
[Writing a document](../README.md).

## Features

Pages are GitHub-flavoured Markdown:

| Feature | Written as |
| --- | --- |
| Tables | pipes, with a `\| --- \|` delimiter row under the header |
| Task lists | `- [ ] todo`, `- [x] done` |
| Footnotes | `text[^1]` and `[^1]: the note` |
| Autolinks | a bare `https://…` address becomes a link |
| Strikethrough | `~~gone~~` |
| Raw HTML | any HTML, passed through unchanged |
| Highlighted code | a fenced block with a language — see [Code blocks](#code-blocks) |
| Diagrams | a fenced block with language `mermaid` — see [Diagrams](../../diagrams/README.md) |
| Widgets | a capitalized HTML tag, `<Callout kind="tip">…</Callout>` — see [Widgets](../../widgets/README.md) |

<Example title="A few of them, as written and as rendered">

| Kind | Read as |
| --- | --- |
| doc | one long page |
| slides | one slide at a time |

- [x] write the design
- [ ] build it

~~Export a copy~~ — the page **is** the file.

<kbd>Raw HTML</kbd> passes through.

</Example>

<Callout kind="warning" title="Raw HTML is not filtered">
Whatever a page contains, including `<script>`, goes into the page as written.
</Callout>

## Code blocks

A fenced code block is syntax-highlighted when its language is one devpage
knows. Name the language after the opening fence — the first word of the info
string counts, so ```` ```sh title ```` is shell:

<Example>

```rust
fn main() {
    println!("live");
}
```

</Example>

The languages are those the `bat` tool ships: syntect's defaults plus others
such as Swift, TypeScript, TOML and YAML. A language is found by its name or
its usual file extension (`rust` or `rs`, `yaml` or `yml`). A block with no
language, or one devpage doesn't know, is shown as plain code; nothing fails.

Highlighting happens on the server when the page renders, and its colors come
from the theme's syntax tokens, so code follows the light and dark themes —
see [Themes](../../themes/README.md#the-tokens). Code
blocks show no language label and have no copy button. A `mermaid` block is
not highlighted; the server draws it as a diagram.

## Images and other files

Any file in the folder that is not Markdown — an image, an SVG, a PDF — is
served as it is, with a content type guessed from its extension. Reference it
with a relative path:

```markdown
![The parts](parts.svg)
```

An image of a `.design` file is not served as bytes: it embeds the design,
drawn live and clickable — `![Checkout](checkout.design)`; see
[Designs](../../designs/README.md#viewing-a-design).

By default a change to any file in the served folder refreshes the open
pages, images included, except files a `.gitignore` excludes. A site can
narrow that, for the files in its own folder, with its `watch` list ([configuration reference](../../configuration-reference/README.md#site)).
