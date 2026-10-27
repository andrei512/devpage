# A version never changes

<Callout kind="note" title="The rule">

Once published, a version's files are the bytes that were uploaded, and
`/d/<id>/v/<n>` shows the same thing for as long as the version exists. devpage
never edits, rewrites or adds to a published file. A new state of a document is
a new version; the only change a version ever sees is being deleted.

</Callout>

## Why

A link someone shared, a comment's "on v3" and a cached download all point at a
version. If a version could change, each would quietly start meaning something
else — and the cache would need invalidation, the one thing that makes a cache
wrong. Keeping versions fixed means a cached copy is valid until it is evicted,
and evicting it costs only a re-read.

## Check

The server stores each upload's zip unchanged and records its sha-256 in
`versions`; nothing writes to a version's bucket object after the publish that
created it. A test publishes, reads the version back from the bucket, and finds
it byte-for-byte the upload, then publishes again and finds the first version
unchanged.
