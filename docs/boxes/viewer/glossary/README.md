# Viewer glossary

The words the viewer owns, one section per term, so any page can link to a
term by its anchor — `glossary/README.md#served-folder`. The rest of the docs use
these words with these meanings. It is the reference page for the
[viewer](../README.md)'s vocabulary.

## document

What devpage serves and a reader opens: a site, a doc, slides or a design. Every kind is a document; the code uses the same word (`Document`, `Kind`).

## kind

Which of the four a document is. A site, a doc or slides is named by the `kind` field of its document file — `site`, `doc` or `slides`; a design is its own `.design` file and has no document file.

## site

A document of many pages with a sidebar built from Topics sections. Configured by `root`.

## doc

A document read top to bottom as one page — a design doc, a PRD — made of one or more chapters, with an outline beside it and PDF settings. The word for the kind is always *doc*, never "document".

## chapter

One Markdown file of a doc, listed in its `chapters` field.

## slides

A document that is a deck: one Markdown file per slide, in the order its `slides` field lists them, each laid out at 1920 × 1080. The kind is *slides*; one deck is a **deck**, one file a **slide**.

## document file

The YAML file describing a document — any name, `devpage.yml` by default.

## entry document

The document devpage was started with, where `/` leads.

## served folder

The folder devpage serves: the one it was started from, or the one `--root` names. Addresses are paths in it, `.devpage/` sits at its top, and nothing outside it is served, read or linked to.

## design

A UI drawn as code: one `.design` file holding one root `Frame`, drawn on a canvas and as a PNG. Never "mockup" or "component"; not to be confused with a design doc, which is a doc, or a design branch, which is a git branch.

## instance

A design used inside another, by the name it was imported as.

## prop

A value a design takes when it is used, declared with a default in its root's `props`.

## node

One element of a resolved design — a frame, text, rectangle, ellipse, vector or image; its **id** is its path of child positions.

## scene

A design resolved into one tree of nodes, imports and props substituted, ready to lay out and paint.

## interaction

What clicking a node does in a prototype: `navigate`, `back`, `overlay` or `close`.

## prototype

Designs linked by interactions, clicked through on the canvas.

## design engine

The `devpage-design` crate: parses, resolves, lays out and paints designs, natively and in WebAssembly.

## page

A Markdown (`.md`) file in the served folder, rendered into a frame when requested. Any other file is served as it is.

## root page

The page a site's `root` names: where the site opens, and the top of its sidebar tree.

## page frame

The HTML a page is wrapped in: the stylesheet, the scripts and the kind's own layout — a site's sidebar, a doc's outline, a deck's stage.

## outline

A doc's navigation: its chapters' headings, beside the text, following the reader as they scroll.

## Topics

A page's `## Topics` section. The first link of each top-level list item names a child of the page in a site's sidebar.

## entry

One page in a sidebar tree, labelled with its Topics link's text.

## group

A run of entries under one `###` heading in a Topics section, shown under that heading in the sidebar.

## live update

An open page fetching itself again and swapping in its content and navigation, in place, after a watched file changed.

## comment mode

The switch beside the theme toggle that turns commenting on: while it is on, selecting text or hovering a paragraph offers a comment. Off until the reader turns it on; the browser remembers it.

## comment

A thread a reader starts, in comment mode, on selected text or on a paragraph: the first message and its replies, one level deep. Stored in the comment store; resolving deletes it.

## anchor

What a comment is attached to: the quoted text, with a little of the text before and after it to tell repeats apart.

## detached comment

A comment whose quoted text is no longer on its page. It stays readable, listed at the top of the page, until it is resolved.

## comment store

`.devpage/comments/` at the top of the served folder: one YAML file per thread. Git never sees it — `.devpage/` ignores itself.

## author

Who wrote a comment: `git config user.name` (or the system's user name) when devpage runs locally, the identity header's value when it is deployed.

## deployed

devpage listening on an address other than the local machine's (`--host`), behind an auth proxy that names the reader in a header.

## problem

A broken link, a missing chapter or slide, a Topics entry pointing nowhere, an invalid document file or a design that doesn't parse or resolve — shown on the page in the preview and reported by `devpage check`.

## print mode

A page rendered for Chrome to print: no live updates, no comments, no navigation chrome. What `devpage pdf` captures.

## built-in asset

One of the files compiled into the binary and served under `/__devpage/`.

## widget

A React component used in Markdown as an HTML tag whose name starts with a capital letter — `<Callout>`. Its props come from the tag's attributes and its content, rendered as Markdown, is its `children`.

## system widget

A widget devpage ships, in every document: `Callout`, `Tabs`, `Tab`, `Example`, `FileTree`, `Terminal`, `Keys`, `Steps`, `Cards` and `Diagram`, through which every Mermaid fence renders.

## project widget

A widget a project writes: one `.tsx` file in the folder its document file's `widgets` field names, named by its file name.

## theme

A set of values for the color and font tokens. There are two, light and dark: by default the reader's system setting picks one, and the theme selector — one button — switches between them.

## navbar

A site's bar across the top: its title, then the links, dropdowns and icons its `navbar` field lists.

## token

A CSS custom property in `assets/style.css` — `--accent`, `--syntax-string` — that the page, its code and its diagrams take their colors and fonts from.
