# Writing a document

How to write a devpage document: the three kinds and when to use each, the
document file that describes one, and the folder devpage serves from. How
pages link to each other and which Markdown renders are on their own pages,
below. It is a task article under the [viewer](../README.md); read
[Getting started](../../../getting-started.md) first for the mental model.

## Three kinds

<KindCards />

A system's documentation is a site. A design doc, a PRD or a proposal is a
doc. A talk is slides. All three are Markdown files on disk, render
the same Markdown, take comments and are checked for broken links; a doc and a
deck also [print to PDF](../pdf/README.md).

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

<FileTree>

- my-project/ — the served folder, when devpage starts here
  - README.md — served at `/README.md`
  - docs/
    - devpage.yml — the site: `devpage docs`
    - README.md — served at `/docs/README.md`
  - src/
    - images/logo.svg — linked from the docs as `../src/images/logo.svg`

</FileTree>

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
first free port from 3030 up. Read the port from the line each one prints:

<Terminal>

```
$ devpage docs
devpage → http://localhost:3030
$ devpage ../my-project/docs
My project → http://localhost:3031
```

</Terminal>

How a request becomes a page is explained in
[Serving pages](../serving-pages/README.md).

## Topics

- [Links between pages and documents](links-between-pages/README.md) — what a
  link to a page, a document file or a heading opens.
- [Markdown that works](markdown-that-works/README.md) — the GitHub-flavoured
  features, code blocks, images and other files.
