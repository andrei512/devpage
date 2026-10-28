# Server reference

The exact contract of server mode: every CLI command, every API request, and
every environment variable. It is the reference page of
[Server mode](../README.md); the tasks are on
[Publishing a document](../publishing/README.md) and
[Running a server](../running-a-server/README.md).

## CLI

| Command | Does |
| --- | --- |
| `devpage context add <name> <server> [--token-command '<cmd>']` | Adds a context. |
| `devpage context use <name>` | Makes it the current context. |
| `devpage context list` | Lists the contexts, the current one marked. |
| `devpage publish <path> [--id <uuid>] [--new]` | Creates the document, or adds a version. Prints its link. |
| `devpage list` | The documents on the server: id, kind, title, latest version, link. |
| `devpage versions <id>` | A document's versions: number, when, who. |
| `devpage comments <id>` | The open threads, grouped by the Markdown file they are on. |
| `devpage comments reply <id> <thread> "<text>"` | Replies to a thread. |
| `devpage comments resolve <id> <thread>` | Resolves a thread. |
| `devpage download <id> [--version <n>] [-o <file>]` | Saves the PDF or PNG of the latest version, or of version `n`. |
| `devpage delete <id> [--version <n>]` | Deletes the document, or one version. |

The server commands — every one but `context` — talk to the current context's
server, and take `--context <name>` to use another, and `--json`. `<id>` is a
document's id on that server. Only `publish` reads files; every other command acts on a document
already on the server.

| File | Holds |
| --- | --- |
| `~/.devpage/config.yml` | `current-context` and `contexts`: each a `server` and an optional `token-command`. |
| `~/.devpage/contexts/<name>/published.yml` | Each published document file's absolute path, and its document's id. Read only by `publish`. |

## API

| Request | Does |
| --- | --- |
| `POST /api/documents?entry=<file>` with a zip | Creates a document: `201 {id, version: 1, url}`. |
| `POST /api/documents/<id>/versions?entry=<file>` with a zip | Adds a version: `201 {version, url}`. |
| `GET /api/documents` | Every document: `[{id, kind, title, latest}]`. |
| `GET /api/documents/<id>` | The document and its versions: `{id, kind, title, latest, created, versions: [{number, kind, title, author, published, size, sha256}]}`. |
| `DELETE /api/documents/<id>` | Deletes it, its versions and its comments: `204`. |
| `DELETE /api/documents/<id>/versions/<n>` | Deletes one version: `204`. |
| `GET /api/documents/<id>/comments` | Its threads, oldest first: `[{id, page, version, detached, detached_in, anchor, thread}]`. |
| `POST /api/documents/<id>/comments` with `{page, anchor, text}` | Starts a thread on the latest version: `201 {id, version}`. |
| `POST /api/documents/<id>/comments/<thread>/replies` with `{text}` | Replies: `201`. |
| `DELETE /api/documents/<id>/comments/<thread>` | Resolves it: `204`. |

`entry` names the document file inside the zip — `devpage.yml`, a deck's
`talk.yml`, a lone `notes.md` or `screen.design` — and is `devpage.yml` when
left out; each version is served from its own. A version's number is never
given again once deleted, so a link to it never shows something else.

A thread's `page` is the Markdown file it is on, from the top of the
document (`/chapters/rollout.md`) — the same in every version, so a thread
carries from one to the next. `version` is the one it was started on;
`detached` says its text is gone from the latest, and `detached_in` is the
version whose publish found it gone (`null` while it is attached). `anchor` is
`{quote, prefix, suffix}` and `thread` its messages, `[{author, at, text}]`,
as in the [comment store](../../comments/the-comment-store/README.md). The
page at `/d/<id>/` reads and writes the same threads through its own comment
requests.

An upload whose check fails gets `422` with its problems; one over
`DEVPAGE_MAX_UPLOAD_BYTES` gets `413`; a zip entry with `..`, an absolute path
or a symbolic link refuses the publish. With `DEVPAGE_USER_HEADER` set, a write
without that header gets `401`.

| Page | Shows |
| --- | --- |
| `/` | That this is a devpage server, how to install the CLI, connect it and publish. It lists no documents. |
| `/favicon.ico` | devpage's logo: the tab icon of every page whose document has no logo of its own. |
| `/d/<id>` | The latest version. |
| `/d/<id>/v/<n>` | Version `n`. |
| `/d/<id>/v/<n>/download` | Its PDF or PNG. |

These routes come before the document's own files: under the latest's mount,
a top-level folder of the document named `v` is not reached as `/d/<id>/v/…`,
and in a version a top-level file named `download` is not reached at
`/d/<id>/v/<n>/download`. A page also asks `__devpage/versions` under its
mount for the version menu, the banner and the download button.

## Environment

| Variable | Default | Meaning |
| --- | --- | --- |
| `DEVPAGE_HOST` | `127.0.0.1` | The address to listen on; `0.0.0.0` in a container. |
| `DEVPAGE_PORT` | `3030` | The port. |
| `DEVPAGE_BUCKET_URL` | required | `s3://bucket/prefix` or `az://container/prefix`; or `file:///folder`, a local folder for trying server mode and for tests — never for a deploy, since a folder on the PVC is lost with it. |
| `DEVPAGE_DATA_DIR` | required | The PVC's mount: the store and the cache. |
| `DEVPAGE_CACHE_MAX_BYTES` | 80% of the PVC | The cache's limit ([the cache](../storage/README.md#the-cache)). |
| `DEVPAGE_CACHE_POLICY` | `lru` | Which entries go when the cache is past its limit ([the eviction policy](../storage/README.md#the-eviction-policy)). |
| `DEVPAGE_MAX_UPLOAD_BYTES` | `100MB` | The largest zip a publish may send. |
| `DEVPAGE_USER_HEADER` | none | The header naming each write's author; unset, writes are `anonymous`. |
| `DEVPAGE_CHROME_SANDBOX` | `on` | `off` in a container, for the PDF downloads: [the configuration reference](../../configuration-reference/README.md#environment-variables). The example image sets it. |
| `AWS_*`, `AZURE_*` | — | The bucket's credentials, read by the SDKs' own chains — so workload identity needs none set. |

On the CLI side, `DEVPAGE_CONTEXT` names the context for one run, as
`--context` does.
