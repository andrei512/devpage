---
id: "01"
blocked-by: []
boxes: [viewer/server, engineering/code-map]
---

# Server mode: configuration, the documents API and a hardened cache

Turn the prototype's `drive/server_mode.rs` into the server's real shape and
finish everything that is not comments, pages or the CLI.

Start by splitting `server_mode` into focused modules — configuration, the
API, the versions (cache + per-version routing), and the storage adapters —
so the lanes after this one each own a file. Lane 02 changes how a version is
routed (the mount) at the same time as you: keep the per-version routing in
one module and leave its referer workaround in place for 02 to delete.

## Build

- **Configuration:** every `DEVPAGE_*` variable in the
  [reference](../boxes/viewer/server/reference/README.md#environment), checked
  before listening; a missing, malformed or bundled-only one stops the server
  naming the variable, never its value. `DEVPAGE_CACHE_MAX_BYTES` defaults to
  80% of the volume under `DEVPAGE_DATA_DIR` (statvfs); `DEVPAGE_CACHE_POLICY`
  takes `lru` only. Bucket credentials come from object_store's own
  `*_from_env` chains, so workload identity needs nothing set.
- **Limits and authors:** an upload over `DEVPAGE_MAX_UPLOAD_BYTES` gets
  `413`; with `DEVPAGE_USER_HEADER` set a write without it gets `401`, and
  each version records its author (`anonymous` without the header).
- **Documents API:** `GET /api/documents`, `GET /api/documents/<id>` (the
  document and its versions), `DELETE /api/documents/<id>` and
  `DELETE /api/documents/<id>/versions/<n>` — deleting the latest makes the one
  before it latest, deleting the only one deletes the document; bucket objects
  and cache entries go with them. A version's number is assigned inside the
  store's transaction, so two concurrent publishes never share one.
- **The cache, production-quality:** seeded from disk on start (recency from
  `mtime`); one unpack per version however many misses arrive at once; an
  entry bigger than the limit served without caching; publish scratch space
  inside the data folder; hits, misses, size and evictions logged.

## Acceptance

- Tests for each variable's failure, the 413, the 401 and authors, every
  documents request, version-number concurrency, and each cache behaviour
  above (the policy stays a pure core function with its own tests).
- The constraint checks pass as tests: [a version never changes](../boxes/viewer/server/constraints/a-version-never-changes.md)
  (byte-for-byte, sha-256 recorded) and the first half of
  [the PVC is only a cache](../boxes/viewer/server/constraints/the-pvc-is-only-a-cache.md)
  (delete the cache, every version renders again from the bucket).
- Code map and tests page updated.
