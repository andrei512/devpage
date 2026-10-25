# Configuration reference

The exact contract of a devpage document file and of the `devpage` command
line: every field of each kind with its type, default and validation, the
commands and flags, and the errors devpage stops with. It is the reference page
under the [viewer](../README.md).

## Command line

```sh
devpage [path] [--root <folder>] [--host <address>] [--port <port>] [--user-header <name>]
devpage check [path] [--root <folder>]
devpage pdf <path> [--root <folder>] [-o <file>]
devpage --help
devpage --version
```

| Command | Does |
| --- | --- |
| `devpage [path]` | Serves the document at `path` and everything in the served folder, until stopped. |
| `devpage check [path]` | Checks the document at `path` and everything reachable from it by links; prints each problem and exits `1` if there was any, `0` if not ([Checking links](../checking-links/README.md)). |
| `devpage pdf <path>` | Prints a doc or a deck to PDF ([Printing to PDF](../pdf/README.md)). |

`path` is read the same way by every command:

| `path` | Means |
| --- | --- |
| a folder | the document file `devpage.yml` inside it |
| a `.md` file | that file, as a doc of one chapter with default settings |
| any other file | a document file, whatever its name |
| none | `.` — `devpage.yml` in the current folder |

### Flags

| Flag | Command | Default | Meaning |
| --- | --- | --- | --- |
| `--root <folder>` | all | the current folder | The [served folder](../writing-a-document/README.md#the-served-folder). The document must be inside it. |
| `--host <address>` | `devpage` | `127.0.0.1` | The address to listen on. Any non-loopback address makes devpage [deployed](../deploying/README.md). |
| `--port <port>` | `devpage` | the first free port from 3030 up | Listen on exactly this port; fail if it is taken. |
| `--user-header <name>` | `devpage` | `X-Forwarded-Email` | The request header naming a comment's author when deployed. Ignored locally. |
| `-o, --output <file>` | `devpage pdf` | `<title>.pdf` in the current folder | Where to write the PDF. |

On start, `devpage` checks the document is inside the
[served folder](../writing-a-document/README.md#the-served-folder), watches it, binds its address and prints one line on standard output — the entry
document's title and its address:

```text
<title> → http://localhost:<port>
```

It then logs every request on standard error (see
[Serving pages](../serving-pages/README.md#request-logging)) and each link
problem once, as it is found. It runs until stopped. Changes to document files
and pages take effect on the next request; nothing needs a restart.

## Document files

A document file is a YAML mapping whose `kind` field says what it describes.
Paths in it are relative to the file's folder and must stay inside the
served folder. A field the kind doesn't have is an error, not ignored.

### `site`

| Field | Type | Required | Meaning |
| --- | --- | --- | --- |
| `kind` | `site` | yes | |
| `title` | string | yes | The site's name: the sidebar heading and the second half of each tab title (`<page> · <title>`). |
| `root` | path | yes | The page the site opens on and the top of its sidebar tree. |
| `watch` | list of glob patterns | no | Narrows what refreshes open pages to the files matching a pattern, relative to the site's folder. Default: every file in the served folder that no `.gitignore` excludes. |

A site's pages are the Markdown files in its document file's folder and below
it; a page there is shown in this site's frame. When sites are nested, a page
belongs to the nearest one.

### `doc`

| Field | Type | Required | Default | Meaning |
| --- | --- | --- | --- | --- |
| `kind` | `doc` | yes | | |
| `title` | string | yes | | The doc's name: above the first chapter, in the tab, and the PDF's title and default file name. |
| `chapters` | list of paths | yes | | The chapter files, in reading order; at least one. |
| `pdf.paper` | `A4` or `Letter` | no | `A4` | The paper size. |
| `pdf.header` | string | no | none | Printed at the top of every page. |
| `pdf.footer` | string | no | `"{page} / {pages}"` | Printed at the bottom of every page. |

In `header` and `footer`, `{page}` is the page's number and `{pages}` the count.

A `.md` file given instead of a document file is a doc whose one chapter is that
file, titled by its first `# ` heading (or its file name), with every `pdf`
default.

### `slides`

| Field | Type | Required | Meaning |
| --- | --- | --- | --- |
| `kind` | `slides` | yes | |
| `title` | string | yes | The deck's name: in the tab, and the PDF's title and default file name. |
| `slides` | list of paths | yes | The slide files, in order; at least one. |

## Errors

devpage stops before serving when the **entry** document can't be loaded. It
prints `Error:` and what it was doing — `reading <file>` or `in <file>` — then
the cause:

| Problem | Message |
| --- | --- |
| The file, or a folder's `devpage.yml`, can't be read | `reading <file>` — then the system error, e.g. `No such file or directory (os error 2)` |
| `kind` missing | `in <file>` — `` missing field `kind` `` |
| `kind` not one of the three | `in <file>` — ``kind: unknown variant `deck`, expected one of `site`, `doc`, `slides` at line 1 column 7`` |
| A field the kind doesn't have | `in <file>` — ``unknown field `root`, expected one of `title`, `chapters`, `pdf` `` |
| A `watch` pattern isn't a valid glob | `in <file>` — `watch pattern <pattern>`, then the glob error, e.g. `error parsing glob 'a[b': unclosed character class; missing ']'` |
| A required field missing, or of the wrong type | `in <file>` — the YAML error, with its line and column |
| `root`, a chapter or a slide isn't an existing file in the served folder | `in <file>` — `<field> <path> does not exist` |
| The document isn't inside the served folder | `<path> is outside the served folder <folder>` |
| `--port` is taken | `binding <address>:<port>` — the system error |
| Every port from 3030 up is taken | `no free port` |

The same problems in a document file reached by a link — not the entry — don't
stop devpage: they are [problems](../checking-links/README.md), shown on the
pages that link to it and reported by `devpage check`. `devpage pdf` also stops
with `pdf prints a doc or slides, not a site` when given a site, and with the
error from fetching Chromium when none is installed and the download fails.

## Examples

devpage's own documentation, `docs/devpage.yml`:

```yaml
# devpage's own documentation, served by devpage.
kind: site
title: devpage
root: README.md
watch:
  - "**/*.md"
  - "**/*.yml"
```

The example doc and deck in `examples/` show the other two kinds.
