# One name, one widget

A widget's name is its file name, it starts with a capital letter, and it names
exactly one widget in a document. A project widget can't share a system
widget's name; two widgets can't share a name; a capitalized tag that names no
widget is not passed through as HTML. Each of these is a problem, never a
silent choice.

**Why:** widgets compose — a system widget renders others by name, and so does
a project's. If a project's `Tabs.tsx` replaced devpage's, every widget built
on `Tabs` would get one with different props and break in ways nobody asked
for, far from the file that caused it. A capitalized tag passed through as HTML
would render nothing and say nothing. Capital letters keep widgets apart from
HTML tags, which are lowercase, as React does.

**Check:** the widget set is built from the system widgets and the document's
widgets folder, and a clash, a lowercase widget file or an unknown capitalized
tag becomes a problem in the preview and in `devpage check`. A test gives a
project a `Callout.tsx` and finds the problem, and the system `Callout` still
rendered.
