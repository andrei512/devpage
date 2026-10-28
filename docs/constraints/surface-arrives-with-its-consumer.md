# Surface area arrives with its consumer

<Callout kind="tip" title="A review rule, written for the AI reviewer">

This file is a constraint for automated AI code review. On every change, whatever it touches, the
reviewer reads it and checks the diff against the rule below; a change that
breaks it is a finding. People can read it too, but it is written to be
checked: the rule, why it holds, and how a breach shows.

</Callout>

<Callout kind="note" title="The rule">

A Markdown extension, a layout or layout option, a CLI command or flag, a
config field, an extension API hook or a CSS hook is added when something needs
it — never speculatively, and never because the implementation makes it easy.

</Callout>

## Why

Every one of them is a contract with the people who write documents
and configure devpage, and their files keep depending on it long after the
change that added it. A contract nobody asked for is one you maintain forever
for free. What devpage supports grows from what documents actually need, so
that what it becomes is decided by its users rather than by what was scaffolded
on day one.

## Check

A diff adding surface names a consumer that exists — a document,
test or feature in the same diff, or something already in the repo. Surface
whose only justification is the ticket that adds it doesn't ship.
