# Metrics name no document

<Callout kind="tip" title="A review rule, written for the AI reviewer">

This file is a constraint for automated AI code review. On every change to this box or to anything inside it, the
reviewer reads it and checks the diff against the rule below; a change that
breaks it is a finding. People can read it too, but it is written to be
checked: the rule, why it holds, and how a breach shows.

</Callout>

<Callout kind="note" title="The rule">

No label on the server's metrics names a document, a version, a page, an
author or anything else a reader or publisher chose: labels are the stage,
the route as the router declares it (`/d/{id}/{*rest}`, never the path asked
for), the status and the document's kind. Metrics are served only on
`DEVPAGE_METRICS_PORT`, never on the port pages are served on.

</Callout>

## Why

A label per document makes a series per document: with 50,000 versions the
metrics grow without bound and the scraper with them. And an id in a label is
a document's link — anyone who can read the metrics could open every document
on the server, which no page of it lists.

## Check

Every label value comes from a closed set fixed in the code. No request to
`DEVPAGE_PORT` answers `/metrics`.
