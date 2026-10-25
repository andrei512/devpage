# Widgets

**Widgets** make a page more visual: React components written as HTML tags in
Markdown. devpage ships a set — callouts, tabs, file trees, terminals, steps,
cards — and a project adds its own as `.tsx` files. This page shows what a
widget is and where each part is explained; it is a concept article under the
[viewer](../README.md).

<Example title="Two widgets, as written and as they render">

<Callout kind="warning" title="Reachable only through the proxy">
devpage **believes** the identity header, so expose the proxy, not devpage.
</Callout>

<Tabs>
<Tab title="crates.io">

    cargo install devpage --locked

</Tab>
<Tab title="From a checkout">

    cargo install --path . --locked

</Tab>
</Tabs>

</Example>

A widget's name starts with a capital letter, so it can never be mistaken for
an HTML tag. Its attributes are its props and the Markdown between its tags is
its content. It works on any page of any kind — a site's page, a doc's chapter,
a slide.

<Cards columns={2}>

- (pointer) [Using widgets](using-widgets/README.md) — props, content, block or
  inline, and what a failing widget looks like.
- (layout-grid) [System widgets](system-widgets/README.md) — every widget devpage
  ships, each shown live beside its source.
- (code) [Writing a project widget](project-widgets/README.md) — a `.tsx` file
  in your widgets folder: composing, helpers, styling, paper.
- (cpu) [How widgets run](how-widgets-run/README.md) — placeholders on the
  server, compiling with oxc, mounting with React.

</Cards>

## Topics

- [Using widgets](using-widgets/README.md) — props, content, block or inline,
  and failures.
- [System widgets](system-widgets/README.md) — the widgets devpage ships, each
  shown live.
- [Writing a project widget](project-widgets/README.md) — a project's own
  widgets in TSX.
- [How widgets run](how-widgets-run/README.md) — from a tag in Markdown to a
  mounted React component.
