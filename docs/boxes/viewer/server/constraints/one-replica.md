# One replica

<Callout kind="tip" title="A review rule, written for the AI reviewer">

This file is a constraint for automated AI code review. On every change to this box or to anything inside it, the
reviewer reads it and checks the diff against the rule below; a change that
breaks it is a finding. People can read it too, but it is written to be
checked: the rule, why it holds, and how a breach shows.

</Callout>

<Callout kind="note" title="The rule">

Server mode runs as exactly one process. Its store is one SQLite file with one
writer, and its cache belongs to that process. Nothing in devpage coordinates
two servers, and the example Kubernetes deploy is a StatefulSet of one.

</Callout>

## Why

Internal documents are read by thousands at most and written far less; one
process serves that. A second writer would need a shared database and caches
that invalidate each other — machinery whose only purpose would be a scale that
never comes, and whose bugs would lose comments.

## Check

There is no database option but SQLite, and no cross-process locking or cache
invalidation in the code. `deploy/server/kubernetes.yaml` sets `replicas: 1`,
and `deploy/server/compose.yaml` runs one server per bucket.
