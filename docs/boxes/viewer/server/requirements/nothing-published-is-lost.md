---
page-type: constraint
id: nothing-published-is-lost
---
# In server mode, a published document, version or comment must never be lost

<Callout kind="note" title="The rule">

In server mode, a published document, version or comment must never be lost.
Losing the server's disk, the pod or the whole machine loses none of them.

</Callout>

## Why

People share a document's link and comment on it. A disk is tied to one zone
and one machine, and gets deleted with a namespace or a mistyped command. If
it held the only copy of a publish or a comment, that would go with it.

## Check

A test runs a server, publishes and comments, deletes its data folder, starts
a server on an empty one against the same bucket, and finds every document,
version and comment. A publish answers only after its zip is in the bucket. Comments are streamed to the bucket about once a second.
