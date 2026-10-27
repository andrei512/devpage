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

Every command talks to the current context's server, and takes
`--context <name>` to use another, and `--json`. `<id>` is a document's id on
that server. Only `publish` reads files; every other command acts on a document
already on the server.

| File | Holds |
| --- | --- |
| `~/.devpage/config.yml` | `current-context` and `contexts`: each a `server` and an optional `token-command`. |
| `~/.devpage/contexts/<name>/published.yml` | Each published document file's absolute path, and its document's id. Read only by `publish`. |

## API

| Request | Does |
| --- | --- |
| `POST /api/documents` with a zip | Creates a document: `201 {id, version: 1, url}`. |
| `POST /api/documents/<id>/versions` with a zip | Adds a version: `201 {version, url}`. |
| `GET /api/documents` | Every document: id, kind, title, latest version. |
| `GET /api/documents/<id>` | The document and its versions. |
| `DELETE /api/documents/<id>` | Deletes it, its versions and its comments. |
| `DELETE /api/documents/<id>/versions/<n>` | Deletes one version. |
| `GET /api/documents/<id>/comments` | Its threads, oldest first. |
| `POST /api/documents/<id>/comments` with `{page, anchor, text}` | Starts a thread on the latest version. |
| `POST /api/documents/<id>/comments/<thread>/replies` with `{text}` | Replies. |
| `DELETE /api/documents/<id>/comments/<thread>` | Resolves it. |

An upload whose check fails gets `422` with its problems; one over
`DEVPAGE_MAX_UPLOAD_BYTES` gets `413`; a zip entry with `..`, an absolute path
or a symbolic link refuses the publish. With `DEVPAGE_USER_HEADER` set, a write
without that header gets `401`.

| Page | Shows |
| --- | --- |
| `/` | That this is a devpage server, and how to connect to it. |
| `/d/<id>` | The latest version. |
| `/d/<id>/v/<n>` | Version `n`. |
| `/d/<id>/v/<n>/download` | Its PDF or PNG. |

## Environment

| Variable | Default | Meaning |
| --- | --- | --- |
| `DEVPAGE_HOST` | `127.0.0.1` | The address to listen on; `0.0.0.0` in a container. |
| `DEVPAGE_PORT` | `3030` | The port. |
| `DEVPAGE_BUCKET_URL` | required | `s3://bucket/prefix` or `az://container/prefix`. |
| `DEVPAGE_DATA_DIR` | required | The PVC's mount: the store and the cache. |
| `DEVPAGE_CACHE_MAX_BYTES` | required | The cache's budget, less than the PVC. |
| `DEVPAGE_MAX_UPLOAD_BYTES` | `100MB` | The largest zip a publish may send. |
| `DEVPAGE_USER_HEADER` | none | The header naming each write's author; unset, writes are `anonymous`. |
| `AWS_*`, `AZURE_*` | — | The bucket's credentials, read by the SDKs' own chains — so workload identity needs none set. |

On the CLI side, `DEVPAGE_CONTEXT` names the context for one run, as
`--context` does.
