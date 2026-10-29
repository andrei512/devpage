# One server holds the envelope

<Callout kind="tip" title="A review rule, written for the AI reviewer">

This file is a constraint for automated AI code review. On every change to this box or to anything inside it, the
reviewer reads it and checks the diff against the rule below; a change that
breaks it is a finding. People can read it too, but it is written to be
checked: the rule, why it holds, and how a breach shows.

</Callout>

<Callout kind="note" title="The rule">

One devpage server on 2 CPUs and 4 GB holds the
[envelope](../measuring-a-server/README.md#the-envelope) at its targets:
10,000 documents and 50,000 versions, 2,000 pages open, 50 views a second and
bursts of 500, a publish and a comment a second — a view under 100 ms at p99,
a publish under 2 s, memory under the limit. A change that makes the
benchmark miss a target is a regression, whatever else it improves.

</Callout>

## Why

[One replica](one-replica.md) rules out adding servers, so a server that
can't hold the load has no way out but to be faster. Without numbers to hold,
a server that holds tens of readers looks like one that holds thousands until
the day a link is posted — and a fix made without them goes where the guess
was, not where the time is.

## Check

The [benchmark](../../../engineering/tests/README.md#the-servers-benchmark)
measures it: `bench.yml` reports each target met or missed. A change to how
a page is served, published, cached or kept in memory runs it before it
merges, and the pull request carries the report's link.
`deploy/server/kubernetes.yaml` sets the pod's requests and limits to the
envelope's 2 CPUs and 4 GB.
