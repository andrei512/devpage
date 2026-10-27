# Four kinds of document

A YAML document file's `kind` says what a folder of Markdown is: a **site** of
pages with a sidebar, a **doc** of chapters read as one page — like this one —
or a deck of **slides**. The fourth kind, a **design**, needs no document
file: a `.design` file is the whole document.

<KindCards />

```yaml
kind: doc
title: devpage features
chapters: [kinds.md, designs.md, markdown.md, widgets.md]
```

Teaches it: [Writing a document](../boxes/viewer/writing-a-document/README.md).
