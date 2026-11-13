---
page-type: constraint
id: nothing-published-is-lost
---
# In server mode, a published document, version or comment must never be lost

<Callout kind="tip" title="A review rule, written for the AI reviewer">

This file is a requirement for automated AI code review. On every change to this box or to anything inside it, the
reviewer reads it and checks the diff against the rule below; a change that
breaks it is a finding. People can read it too, but it is written to be
checked: the rule, why it holds, and how a breach shows.

</Callout>

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
version and comment. A publish answers only after its zip is in the bucket.

## Not met yet

A comment is saved to the store on the disk and copied to the bucket about a
second later, so a disk lost in that second loses the comment. Closing the gap
means a comment is answered only after it is off the disk.
