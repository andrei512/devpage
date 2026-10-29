# A version's work is done once

<Callout kind="tip" title="A review rule, written for the AI reviewer">

This file is a constraint for automated AI code review. On every change to this box or to anything inside it, the
reviewer reads it and checks the diff against the rule below; a change that
breaks it is a finding. People can read it too, but it is written to be
checked: the rule, why it holds, and how a breach shows.

</Callout>

<Callout kind="note" title="The rule">

Whatever server mode works out from a version's files — a page's problems,
its search index, its downloads — it works out at most once while the
version is in memory, and every later request uses the result. A view never
does work whose answer the last view of the same page already had.

</Callout>

## Why

A version never changes ([a version never changes](a-version-never-changes.md)),
so any work redone on it gives the answer it gave last time. Redone on every
view, that work is the server's ceiling: checking a page's links on every view
took nearly all of two CPUs at 200 views a second, where the rest of a view
took a hundredth of it.

## Check

In server mode, a page view of a version already viewed calls no check, and a
search of a version already searched builds no index; the
[benchmark](../../../engineering/tests/README.md#the-servers-benchmark) shows
`check` once per page, not once per view.
