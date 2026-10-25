# Writing a doc

How to write a **doc** — a design doc, a PRD, a proposal: a document read top to
bottom, like a Word file. It covers splitting a doc into chapters, the outline
that follows the reader, links between chapters, and the paper, header and
footer its PDF gets. It is a task article under the [viewer](../README.md); the
document file in general is on [Writing a document](../writing-a-document/README.md).

## One file or chapters

The smallest doc is one Markdown file, with no document file at all:

```sh
devpage design.md
devpage pdf design.md
```

It is titled by its `# heading` and printed with the default PDF settings.

A longer doc is split into **chapters**, one Markdown file each, listed in
order in a document file:

```yaml
kind: doc
title: Sync design
chapters:
  - context.md
  - design.md
  - rollout.md
```

The chapters are read as one page, in the order listed. Reordering them is
moving a line; nobody edits one huge file. Each chapter starts with its own
`# heading`, which becomes a chapter heading in the doc; `title` is the doc's
name, shown above the first chapter, in the tab and in the PDF's metadata. A
doc made from one Markdown file has no separate title: its `# heading` is the
title. A
chapter can be any Markdown file in the repository, so one shared chapter — a
glossary, a context section — can appear in several docs.

## The outline

Beside the text, the doc shows its **outline**: each chapter's heading and the
`##` headings under it. It stays in view as the reader scrolls and highlights
the section they are in; clicking an entry scrolls there. The outline is the
doc's table of contents — there is no other one to write. On a narrow screen it
moves above the text.

## Links between chapters

Write links between chapters as links between their files, as you would in the
repository:

```markdown
The rollout is in [three steps](rollout.md#steps).
```

In the doc, that link scrolls to the *Steps* heading of the rollout chapter.
In a doc, a heading's id is its chapter's file name, without `.md`, then `--`,
then the heading's own id — `rollout--steps` — so two chapters can both have a
`## Risks` section and links to each still land in the right one. Links between
chapters, and `#heading` links within one, are rewritten to those ids when the
doc renders. A link from outside the doc to a chapter file opens that file on
its own, as a page; link to the doc's document file to open the doc.

## On paper

A doc prints to PDF with `devpage pdf` ([Printing to PDF](../pdf/README.md)),
or with the browser's own print. Its `pdf` field sets the paper and what is
printed above and below every page:

```yaml
kind: doc
title: Sync design
chapters: [context.md, design.md, rollout.md]
pdf:
  paper: Letter                         # A4 (default) or Letter
  header: Sync design — draft           # default: none
  footer: "{page} / {pages}"            # default: "{page} / {pages}"
```

`{page}` is the page's number and `{pages}` the count. Page numbers, header and
footer exist only on paper — the doc in the browser is one continuous page.
Every field of `pdf` is optional; the exact contract is in the
[configuration reference](../configuration-reference/README.md#doc).

## Comments and problems

Comments on a doc attach to the chapter file the text came from, so they
survive a reordering ([Comments](../comments/README.md)). A missing chapter,
or a broken link in any chapter, is listed at the top of the doc while you
write ([Checking links](../checking-links/README.md)).
