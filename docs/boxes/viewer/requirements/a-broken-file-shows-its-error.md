---
page-type: constraint
id: a-broken-file-shows-its-error
---
# When a file is broken, the page must show the error

<Callout kind="tip" title="A review rule, written for the AI reviewer">

This file is a requirement for automated AI code review. On every change to this box or to anything inside it, the
reviewer reads it and checks the diff against the rule below; a change that
breaks it is a finding. People can read it too, but it is written to be
checked: the rule, why it holds, and how a breach shows.

</Callout>

<Callout kind="note" title="The rule">

When a file is broken, the page must show the error. A page never looks fine
while a file behind it can't be read, parsed or prepared. What else the page
shows is each kind's choice: a design keeps its last good drawing beside the
error.

</Callout>

## Why

A page that hides the error shows old or missing content as if it were
current, and the author finds out from a reader.

## Check

A test renders a diagram that does not parse and finds its error and its
source on the page. The extension test removes an extension's input, requires
`prepare` and `view` to fail, then restores it and requires recovery; a failed
preparation is never served as current.
