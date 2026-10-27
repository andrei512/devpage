# The PVC is only a cache

<Callout kind="note" title="The rule">

In server mode, nothing on the PVC is the only copy of anything. Version zips
are in the bucket before the publish answers; the store is replicated to the
bucket by Litestream; everything else on the PVC — unpacked versions, compiled
widgets, downloads — is rebuilt from those on demand. Losing the PVC loses at
most the store's last second of writes.

</Callout>

## Why

A PVC is tied to one zone and one disk, and gets deleted with a namespace or a
mistyped command. If it held the only copy of a publish, that publish would go
with it. Writing to the bucket first makes the PVC disposable: a pod on a new,
empty volume restores the store and refills the cache by itself.

## Check

A publish answers only after its zip is in the bucket. A test runs a server,
publishes and comments, deletes its data folder, starts a server on an empty one
against the same bucket, and finds every document, version and comment.
