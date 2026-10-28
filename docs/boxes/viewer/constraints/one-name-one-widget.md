# One name, one widget

<Callout kind="tip" title="A review rule, written for the AI reviewer">

This file is a constraint for automated AI code review. On every change to this box or to anything inside it, the
reviewer reads it and checks the diff against the rule below; a change that
breaks it is a finding. People can read it too, but it is written to be
checked: the rule, why it holds, and how a breach shows.

</Callout>

<Callout kind="note" title="The rule">

A widget's name is its file name, which starts with a capital letter, and it
names exactly one widget in a document. A project widget can't share a system
widget's name; two widgets can't share a name; a capitalized tag that names no
widget is not passed through as HTML. Each of these is a problem, never a
silent choice.

</Callout>

## Why

Widgets compose — a system widget renders others by name, and so does
a project's. If a project's `Tabs.tsx` replaced devpage's, every widget built
on `Tabs` would get one with different props and break in ways nobody asked
for, far from the file that caused it. A capitalized tag passed through as HTML
would render nothing and say nothing. Capital letters keep widgets apart from
HTML tags, which are lowercase, as React does.

## Check

The widget set is built from the system widgets and the capitalized
files of the document's widgets folder, and a clash or an unknown capitalized
tag becomes a problem in the preview and in `devpage check`. A test gives a
project a `Callout.tsx` and finds the problem, and the system `Callout` still
rendered.
