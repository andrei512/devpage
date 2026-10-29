# Publishing a document

How to publish a document to a devpage server and keep it up to date from the
CLI: connecting to a server, the first publish, updates, what a publish
contains, and deleting. It is a task article under [Server mode](../README.md);
every command and flag is in the [reference](../reference/README.md#cli).

## Connect to a server

```sh
devpage context add work https://docs.company.com
devpage context use work
```

If a proxy guards the server, give the context a **token command**. devpage
runs it before each command and sends what it prints as a bearer token; the
command caches and refreshes its own token, so devpage stores none. With Entra
ID in front:

```sh
devpage context add work https://docs.company.com \
  --token-command 'az account get-access-token --resource api://devpage-docs --query accessToken -o tsv'
```

The contexts are kept in `~/.devpage/config.yml`. `--context <name>` uses
another one for a single command.

## Publish and update

```sh
devpage publish launch/devpage.yml    # the first time: creates the document, prints its link
devpage publish launch/devpage.yml    # after: adds a version to the same document
```

The path is read as every devpage command reads it: a document file, a folder
holding `devpage.yml`, a `.md` file or a `.design` file.

**The CLI remembers which document a file is**, per context, in
`~/.devpage/contexts/<name>/published.yml` — never in the document. Two
documents in one folder don't collide, and the same file published to two
contexts is two documents.

- `--id <uuid>` publishes to that document, whatever the CLI remembers.
- `--new` always creates a new document.
- **In CI**, every run is a fresh checkout with nothing remembered, so pass
  `--id`.

## What a publish contains

Exactly what the document is made of — the same files
[`devpage check`](../../checking-links/README.md) follows: the document file,
its pages, chapters or slides, the project widgets they use, every local
image they link to, and every file a link reaches — the check follows links
from page to page, so a document that links into the rest of a repository's
docs carries them too. Drafts or secrets lying in the same folder that nothing
links to are never sent.

If a link is broken, the publish stops before uploading anything and lists the
problems. A publish lands whole or not at all: the new version and its
comments [carried onto it](../reading-a-published-document/README.md#comments)
are recorded together, so one that fails leaves the document as it was, and
publishing again takes the same version number. The server runs the same check again on the files it received, so a
hand-made zip can't publish one either — every check but external addresses,
which the CLI asks before uploading, so that a publish never waits on the
internet from inside the cluster.

## Everything else takes the id

Publishing is the only command that reads files. Once a document exists, every
other command names it by its **id** — the one `publish` printed — and talks to
the current context's server:

```sh
devpage versions 6f1c2a9e-…
devpage comments 6f1c2a9e-…
```

## Delete

```sh
devpage delete 6f1c2a9e-…               # the document, its versions and comments
devpage delete 6f1c2a9e-… --version 3   # one version
```

Deleting the latest version makes the one before it the latest; deleting the
only version deletes the document.

## For agents

Every command takes `--json`. The loop an agent runs on a published document:
`devpage comments <id>` to read the threads, edit the source, `devpage publish`,
then `devpage comments resolve <id> <thread>` for each thread it dealt with. What it sees is
on [Reading a published document](../reading-a-published-document/README.md#comments).
