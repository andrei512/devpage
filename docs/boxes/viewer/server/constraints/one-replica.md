# One replica

<Callout kind="note" title="The rule">

Server mode runs as exactly one process. Its store is one SQLite file with one
writer, and its cache belongs to that process. Nothing in devpage coordinates
two servers, and the example deploy is a StatefulSet of one.

</Callout>

## Why

Internal documents are read by thousands at most and written far less; one
process serves that. A second writer would need a shared database and caches
that invalidate each other — machinery whose only purpose would be a scale that
never comes, and whose bugs would lose comments.

## Check

There is no database option but SQLite, and no cross-process locking or cache
invalidation in the code. The example deploy sets one replica.
