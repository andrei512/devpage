# Writing a document

How to write a devpage document: the four kinds and when to use each, the
document file that describes one, the folder devpage serves from, how
pages and documents link to each other, and which Markdown features render. It
is a task article under the [viewer](../README.md); read
[Getting started](../../../getting-started.md) first for the mental model.

## Four kinds

<KindCards />

A system's documentation is a site. A design doc, a PRD or a proposal is a
doc. A talk is slides. A screen or a flow an agent proposes is a design. The
first three are Markdown files on disk, render the same Markdown, take comments
and are checked for broken links; a doc and a deck also
[print to PDF](../pdf/README.md). A design is not Markdown and has no document
file: its `.design` file is the whole document, and a page embeds it with
`![Name](name.design)` — see [Designs](../designs/README.md).

## The document file

A document is described by one YAML file. Its `kind` says which of the three it
is, and the rest of its fields follow from that:

<Tabs>
<Tab title="Site">

```yaml
kind: site              # a site of pages
title: devpage          # its name, shown in the sidebar and the tab
root: README.md         # the page it opens on
```

</Tab>
<Tab title="Doc">

```yaml
kind: doc
title: Sync design
chapters: [context.md, design.md, rollout.md]
```

</Tab>
<Tab title="Slides">

```yaml
kind: slides
title: Launch
slides: [intro.md, problem.md, demo.md]
```

</Tab>
</Tabs>

The file can have any name. `devpage.yml` is the one devpage looks for when
it is given a folder, so name it that unless a folder holds several
documents. Paths in it are relative to its folder. A change to a document file
takes effect on the next request; there is nothing to restart. Every field,
its type and its errors are in the
[configuration reference](../configuration-reference/README.md); a doc's
chapters and PDF settings are on [Writing a doc](../writing-a-doc/README.md),
a deck's slides on [Making slides](../making-slides/README.md).

**A single Markdown file needs no document file.** `devpage design.md` serves
it as a doc of one chapter, titled by its `# heading`, with the default PDF
settings. Give it a document file when it grows chapters or needs a header or
footer on paper.

## The served folder

devpage serves from the **served folder**: the folder you start it from, or
the one `--root` names. Start it from the top of the repository and the whole
repository is served, whichever document you open:

```sh
cd my-project && devpage docs              # serves my-project/
devpage --root ~/code/my-project ~/code/my-project/docs
```

<Callout kind="warning" title="The served folder is the boundary">
Everything in the served folder can be served, and nothing outside it can: not
through `..`, not through a symbolic link ([the rule](../constraints/the-served-folder-is-the-boundary.md)).
</Callout>

The document you start devpage with must be inside the served folder. A
file's address is its path from the top of the served folder:
`docs/boxes/viewer/README.md` is served at `/docs/boxes/viewer/README.md`, and
`/` leads to the document devpage was started with. A folder has no address of
its own; link to a file in it instead.

A page's title — shown in the browser tab — is its first line that starts with
`# `; a page without one is titled with its file name. Start every page with
`# Title` so the tab reads well.

## Links between pages and documents

Because addresses are file paths, a relative link between files works
exactly as it does when the files are read on disk — including one
that climbs out of the docs folder to the repository's `README.md`:

```markdown
See [live updates](../live-updates/README.md#the-page-script).
Read the [sync design](../../design/sync/devpage.yml) or watch the
[launch talk](../../talks/launch/devpage.yml#3).
```

- **A link to a Markdown file** opens it as a page of the document it belongs
  to: a page inside a site's folder is shown in that site, with its sidebar; a
  page that belongs to no site is shown as a one-chapter doc.
- **A link to a document file** opens that document as its kind — a site at its
  root page, a doc whole, a deck at its first slide, or at slide *n* with
  `#n`.
- **A link to a heading.** Every heading gets an id from its text
  (`## Two words` → `#two-words`), so links can point at a section. Within a
  doc, links between its chapters and to their headings scroll within the doc.

A link within the same document opens in place, without a full reload. Which
links appear in a site's sidebar is decided only by its pages' `## Topics`
sections — see [Organizing navigation](../organizing-navigation/README.md).
Every link is checked: a broken one is marked on the page and listed at its
top while you write ([Checking links](../checking-links/README.md)).

## Markdown that works

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
| Diagrams | a fenced block with language `mermaid` — see [Diagrams and themes](../diagrams-and-themes/README.md) |
| Widgets | a capitalized HTML tag, `<Callout kind="tip">…</Callout>` — see [Widgets](../widgets/README.md) |

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
see [Diagrams and themes](../diagrams-and-themes/README.md#the-themes). Code
blocks show no language label and have no copy button. A `mermaid` block is
not highlighted; it becomes a diagram.

## Images and other files

Any file in the folder that is not Markdown — an image, an SVG, a PDF — is
served as it is, with a content type guessed from its extension. Reference it
with a relative path:

```markdown
![The parts](parts.svg)
```

By default a change to any file in the served folder refreshes the open
pages, images included, except files a `.gitignore` excludes. A site can
narrow that, for the files in its own folder, with its `watch` list ([configuration reference](../configuration-reference/README.md#site)).

## Documenting another repository

devpage serves any repository's Markdown, not only its own. Give the other
repository a document file — usually `docs/devpage.yml` with `kind: site` — and
start devpage from that repository's top:

```sh
cd ../my-project && devpage docs                  # the folder holding devpage.yml
devpage --root ~/code/my-project ~/code/my-project/docs/devpage.yml
```

Pages can link anywhere in the served folder, so the docs can stay in `docs/`
and still link to the top-level `README.md` or to images beside the code.

<Callout kind="warning">
Remember that everything in the served folder can be served to whoever can reach
the address — only the local machine, unless devpage is
[deployed](../deploying/README.md).
</Callout>

**Several at once.** Each devpage process serves one folder and takes the
first free port from 3030 up, so a second one started while the first runs
prints `http://localhost:3031`, the next `3032`, and so on. Read the port from
the line each one prints.

How a request becomes a page is explained in
[Serving pages](../serving-pages/README.md).
