# A PDF is the page the browser shows

<Callout kind="tip" title="A review rule, written for the AI reviewer">

This file is a constraint for automated AI code review. On every change to this box or to anything inside it, the
reviewer reads it and checks the diff against the rule below; a change that
breaks it is a finding. People can read it too, but it is written to be
checked: the rule, why it holds, and how a breach shows.

</Callout>

<Callout kind="note" title="The rule">

`devpage pdf` makes a PDF by having Chrome print the page devpage serves, in
**print mode**. Print mode differs from the screen only by what it leaves out
— live updates, comments, a site's sidebar or a doc's outline, a deck's
controls, soft shadows — and by its `@page` size. It renders the same Markdown with the same
stylesheet, the same highlighted code and the same diagrams. There is no second
renderer for paper.

</Callout>

## Why

A second renderer is a second implementation of every Markdown
feature, every diagram and every theme token, and the two drift: the PDF a
reviewer signs off is no longer the page the author wrote. A deck laid out at a
fixed 1920 × 1080 and only scaled to the window prints exactly as it shows;
one that reflows to the window does not.

## Check

Print-mode differences live in the stylesheet's `@media print` rules
and in leaving parts out of the frame — never in a separate HTML path or
Markdown pass. A test prints the example doc and the example deck and finds
one page per slide at 16:9, and the doc's header and footer on its pages.
