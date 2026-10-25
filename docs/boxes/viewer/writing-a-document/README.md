# Writing a document

How to write a devpage document: the `devpage.yml` that describes it, where its
pages live, how pages link to each other, which Markdown features render,
how code blocks and images are shown, and how to give another repository's
docs a `devpage.yml`. It is a task article under the
[viewer](../README.md); read [Getting started](../../../getting-started.md)
first for the mental model.

## The document file

A document is one YAML file, usually named `devpage.yml`:

```yaml
title: devpage          # the document's name, shown in the sidebar and the tab
root: README.md         # the page "/" opens
watch:                  # optional; defaults to every Markdown file
  - "**/*.md"
```

- `title` is the document's name. It heads the sidebar and follows the page's title
  in the browser tab (`Planting · Garden`).
- `root` is the page the address `/` redirects to, and the top of the sidebar
  tree. It must be an existing file.
- `watch` lists glob patterns for the files whose changes refresh open pages.
  Leave it out to watch every Markdown file.

Paths are relative to the YAML file's folder. The file is read once, at start;
after changing it, restart devpage. Exact types, defaults and errors are in the
[configuration reference](../configuration-reference/README.md).

## Where pages live

Pages are the Markdown (`.md`) files in the YAML file's folder and any folder
below it. Nothing outside that folder is served — not through `..`, and not
through a symbolic link that points outside.

A page's address is its path inside the folder: `boxes/viewer/README.md` is
served at `/boxes/viewer/README.md`. A folder has no address of its own;
link to its `README.md` (or whatever file it holds) instead.

A page's title — shown in the browser tab — is its first line that starts with
`# `; a page without one is titled with its file name. Start every page with
`# Title` so the tab reads well.

## Links between pages

Because addresses are file paths, a relative link between Markdown files works
exactly as it does when the files are read in the repository:

```markdown
See [live updates](../live-updates/README.md#the-page-script).
```

Every heading gets an id from its text (`## Two words` → `#two-words`), so
links can point at a section. A link to another page of the document opens it
in place, without a full reload; a link with only a `#fragment` scrolls within
the page.

Which links appear in the sidebar is decided only by the page's `## Topics`
section — see [Organizing navigation](../organizing-navigation/README.md).

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

Raw HTML is not filtered: whatever a page contains, including `<script>`, goes
into the page as written.

## Code blocks

A fenced code block is syntax-highlighted when its language is one devpage
knows. Name the language after the opening fence — the first word of the info
string counts, so ```` ```sh title ```` is shell:

````markdown
```rust
fn main() {}
```
````

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

Changes to such files refresh open pages only if a `watch` pattern matches
them; the default watches Markdown alone.

## Documenting another repository

devpage serves any repository's Markdown, not only its own. Give the other
repository a `devpage.yml` and point devpage at it.

**Choose where the YAML file goes.** Its folder is the boundary of what is
served: every page, every image and every page a Topics list names must sit in
that folder or below it. A link that climbs out of it never reaches the file it means: the
browser can't go above `/`, so `../assets/logo.png` becomes `/assets/logo.png`
and gets `404 … is not part of this document`, and `../README.md` from the
root page becomes `/README.md`, the document's own root page. A Topics link to
it is left out of the sidebar. So:

- If the docs are self-contained in `docs/`, put `docs/devpage.yml` there,
  with `root: README.md` (or whichever page is the entrance).
- If they link to the repository's top-level `README.md`, or to images
  outside `docs/`, put `devpage.yml` at the repository root and give the root
  page's path from there, for example `root: docs/README.md`.

Remember that everything in that folder is served to whoever can reach the
address — only the local machine, since devpage binds `127.0.0.1` — source
files and `devpage.yml` included.

**Narrow `watch` when the folder is large.** The watcher always watches the
whole folder, recursively; `watch` only decides which changes refresh pages.
At a repository root the default `**/*.md` also matches Markdown in build
output or vendored folders, so name the folders you mean, and add the images
your pages show:

```yaml
title: My project
root: docs/README.md
watch:
  - "docs/**/*.md"
  - "README.md"
  - "docs/**/*.svg"
```

**Run it** with the YAML file or its folder, as a relative or an absolute
path:

```sh
devpage ../my-project/docs              # the folder holding devpage.yml
devpage ~/code/my-project/devpage.yml   # the file itself
```

**Several documents at once.** Each devpage process serves one document and
takes the first free port from 3030 up, so a second one started while the
first runs prints `http://localhost:3031`, the next `3032`, and so on. Read the
port from the line each one prints; a document gets a different port from one
run to the next if another devpage started first.

How a request becomes a page is explained in
[Serving pages](../serving-pages/README.md).
