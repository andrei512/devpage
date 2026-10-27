# Prototype: server mode

Wall clock: 22:21 → 22:37, about **16 minutes** of the 30.

**What runs:** `devpage server` reads `DEVPAGE_DATA_DIR`, `DEVPAGE_BUCKET_URL`,
`DEVPAGE_CACHE_MAX_BYTES`, `DEVPAGE_HOST` and `DEVPAGE_PORT`. The flow:

1. `POST /api/documents` with a zip unpacks it into a scratch folder and checks
   it. A broken link, or an entry outside the zip, gets `422` with the problems.
2. The zip goes unchanged to `documents/<id>/versions/1.zip` in the bucket, then
   rows are written to SQLite (`documents`, `versions`).
3. `POST /api/documents/<id>/versions` adds version 2.
4. `GET /d/<id>` redirects into the latest version's root page.
5. `GET /d/<id>/v/<n>/…` builds the version on a cache miss: it fetches the zip
   from the bucket and unpacks it into `$DATA/cache/<id>/<n>`. It then builds
   that version's own `server::router` and serves from it.
6. When the cache is past its limit, it asks an `Eviction` policy (`Lru`, in
   core) what to delete. It never deletes the version being served.

**Backends tried:**

- `file://` works, which is what the tests use.
- `az://` against Azurite works by hand: publish, then render from a cold cache.
- S3 was not tried. The MinIO image pull failed, as anonymous pulls from Docker
  Hub and quay.io were refused.

**Seen in chrome-headless-shell:** a 10-page site published as a zip renders
whole: sidebar, Mermaid diagram, Cards widget, and every sidebar link returns
200.

## What it touched

- **core:**
  - `cache.rs` (new): `Entry`, the `Eviction` port, the `Lru` policy. Pure,
    tested on lists of entries.
  - `served.rs`: a `ServedFolder` now has a mount. `url()` adds the mount, and
    a new `name()` gives the unmounted path for labels.
  - `document.rs`: `name()`.
  - `navbar.rs` and `check.rs`: these used `url(..)[1..]` as a label; they now
    use `name()`.
- **binary:**
  - `adapters/bucket.rs`: `object_store` behind `parse_url_opts`, with the
    `AWS_*` and `AZURE_*` variables passed through.
  - `adapters/sqlite.rs`: rusqlite, bundled.
  - `drive/server_mode.rs`: config, API, cache, and per-version routers.
  - `drive/server.rs`: `asset` made `pub`, and the `name()` call sites.
  - `main.rs`: the `server` subcommand.
- **New dependencies:** rusqlite (bundled), object_store (`aws`, `azure`), zip,
  uuid, url, sha2.

## Where the difficulty actually was

Not in storage. Bucket, SQLite, zip and cache took minutes.

**It was the mount prefix.** Stage 1 gave the resolver a `mount`, but that only
prefixes redirects. Every address a page carries is produced elsewhere:
`Document::url` via `ServedFolder::url`, the sidebar, the navbar, and widget
addresses. All of them were absolute from `/`.

1. **The mount has to live on `ServedFolder`, and it gets lost.**
   `Document::load(path, served: impl AsRef<Path>)` and
   `ServedFolder::canonical(path)` rebuild a served folder from a bare path
   in about 15 places. Among them are the index, the entry's reload,
   `check.rs`'s linked documents, the module snapshot and the design store, so
   a mount set on one value never reaches the documents loaded from it. The
   prototype works around this with a process-wide `path → mount` registry
   (`ServedFolder::mount_at`), marked PROTOTYPE. The real fix is to pass
   `&ServedFolder` everywhere a document or snapshot is made from one. It is
   the "one owner lookup instead of reloading documents in four modules" item
   stage 1 deferred: stage 2 now needs it.
2. **`url()` did two jobs.** It gave an address and, through `[1..]`, a label
   (problem files, navbar keys, widget labels). With a mount those split into
   `url()` and `name()`. Four call sites moved; others may still assume
   `url()` starts at `/`.
3. **The page script and widgets ask for absolute endpoints.** These are
   `/__devpage/events`, `/__devpage/comments`, `/__devpage/search`,
   `/__devpage/system-widgets/…`, `/__devpage/widgets/…`, `/__devpage/design`
   and `/devpage.design.v1.Designs/Watch`. Only built-in assets are
   document-independent. The prototype routes every other request by its
   `Referer` to the asking page's version (PROTOTYPE). The real fix is for the
   page to know its mount: `addresses::address` and the page script prefix it
   for everything but built-in assets.
4. **axum leaks path parameters across nested routers.** Forwarding a request
   from `/d/{id}/v/{n}/{*rest}` into a version's router carried the outer
   `Path` params in the extensions. The inner `Path<String>` then failed with
   "expected 1, got 4". The fix is to clear the extensions before forwarding.

## What the design got wrong or left open

- **`/d/<id>` cannot be served in place.** A page's relative links resolve
  against its own URL, so the latest version is a redirect to
  `/d/<id>/v/<n>/<root page>`. The address bar then shows the version, which
  contradicts "the link always shows the latest" only in what the bar displays.
  Serving in place would need every address on the page written relative to
  `/d/<id>/`, with no version in them. Needs a ruling: redirect (what runs), or
  a `latest` mount.
- **Default cache limit.** "80% of the PVC" needs the volume's size, through
  statvfs or `fs4`. The prototype defaults to 1 GiB (marked TODO).
- **Credentials:** the SDK variables are passed lowercased to
  `parse_url_opts`. That works for `AZURE_STORAGE_USE_EMULATOR` and the account
  name, but object_store's builders use their own `*_from_env` chains, and
  workload identity isn't tested. The design's "read by the SDKs' own chains" is
  roughly true, not exactly.
- **Not checked:** the design says the server runs "the same check" as the CLI.
  The publish check here is `check::document` with no external checker, so
  external links are not checked on the server. Probably right, since a publish
  shouldn't wait on the internet, but the design doesn't say.

## What is left to be production-quality

- **Mount:** thread `&ServedFolder` through `Document::load`, the index,
  snapshots and designs. Delete the registry and the referer routing. Then
  make widget, event, comment, search and design-stream addresses carry the
  mount, and test them in Chrome.
- **Configuration:** every `DEVPAGE_*` variable, strict at startup (no bundled
  ones), the 80%-of-volume default, `DEVPAGE_MAX_UPLOAD_BYTES` (413; axum's
  2 MB body default currently applies), `DEVPAGE_CACHE_POLICY`, and
  `DEVPAGE_USER_HEADER` (401, authors).
- **Store:** `threads` and `messages`, the comment `Store` port's SQLite
  adapter, and re-anchoring on publish (detached threads). Deletion of
  documents and versions. `GET /api/documents` and `GET /api/documents/<id>`.
- **Cache:**
  - seeded from disk on start (it is only in memory now);
  - sizes counted for downloads too;
  - an entry bigger than the limit served without caching;
  - two concurrent misses on one version (now both unpack);
  - publish's scratch folder inside the cache dir;
  - logging hits, misses and evictions;
  - eviction happens on insert only.
- **Live update:** open pages swap in a new version on publish, through the
  change bus.
- **Pages:** `/` (what this server is), the version menu and old-version banner,
  downloads (PDF and PNG).
- **CLI:** contexts, `token-command`, `publish` (zip from the check graph),
  `list`, `versions`, `comments`, `download`, `delete`, `--json`, `published.yml`.
- **Deploy:** `deploy/server/` and `deploy/bundled/` (Dockerfile, litestream.yml,
  compose with Azurite and an S3 emulator, which needs an image that pulls), and
  the restore test from the PVC constraint.
- **Docs:** code map, tests page, glossary, and a showcase slide.
- **Tests not written:** S3, a browser test of a published page with widgets,
  a SHA mismatch, concurrent publishes to one document (version numbers come
  from `latest + 1` outside the transaction).
