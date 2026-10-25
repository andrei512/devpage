# Configuration reference

The exact contract of a devpage document file and of the `devpage` command
line: every `devpage.yml` field with its type, default and validation, the
errors devpage stops with, and how its argument is interpreted. It is the
reference page under the [viewer](../README.md); the fields are defined by
`Config` in `src/document.rs` and the command line in `src/main.rs`.

## Command line

```sh
devpage [document.yml | folder]
```

| Argument | Meaning |
| --- | --- |
| a folder | the document file is `devpage.yml` inside that folder |
| anything else | the document file itself, whatever its name |
| none | `.` — `devpage.yml` in the current folder |

Only the first argument is read; there are no flags. From the repository,
`cargo run -- <document.yml | folder>` runs the same program.

On start devpage loads the document file once, starts watching its folder, and
binds the first free port from 3030 up on `127.0.0.1`. It prints one line on
standard output:

```text
<title> → http://localhost:<port>
```

and then logs every request on standard error (see
[Serving pages](../serving-pages/README.md#request-logging)). It runs until
stopped. Changes to the document file need a restart; changes to pages don't.

## The document file

A YAML mapping with these fields. The **document's folder** is the folder
holding the file; relative paths are resolved against it, and only files inside
it are served.

| Field | Type | Required | Default | Meaning |
| --- | --- | --- | --- | --- |
| `title` | string | yes | — | The document's name: the sidebar heading and the second half of each tab title (`<page> · <title>`). |
| `root` | string (path) | yes | — | The page `/` redirects to and the top of the sidebar tree. Must name an existing file. |
| `watch` | list of strings (glob patterns) | no | `["**/*.md"]` | Files whose changes refresh open pages. |

No other field is allowed.

### `root`

A path relative to the document's folder, such as `README.md` or
`guide/index.md`. devpage checks that it is an existing file when it starts.
It is served at `/<root>` like any other file, so a root that isn't a Markdown
file is sent as it is rather than rendered as a page.

### `watch`

A list of glob patterns, matched against each changed file's path relative to
the document's folder (for example `boxes/viewer/README.md`). A change to a
file matching any pattern refreshes every open page. Patterns use `globset`
syntax with its defaults:

- `*` matches any run of characters, **including `/`** — `*.md` matches
  `boxes/viewer/README.md` too;
- `**` matches any number of folders;
- `?`, `[abc]` character classes and `{a,b}` alternatives are supported.

The default, `**/*.md`, watches every Markdown file. Add patterns for other
files whose changes should refresh pages, for example `"**/*.svg"`. The value
must be a YAML list, even for one pattern. `watch` does not limit what is
served: every file in the folder is served whether or not it is watched.

## Errors

devpage exits with an error, before serving anything, when it can't start. It
prints `Error:` and what it was doing — for the document file, `reading <file>`
or `in <file>` — then the cause under `Caused by:`:

| Problem | Message |
| --- | --- |
| The file or folder's `devpage.yml` can't be read | `reading <file>` — then the system error, e.g. `No such file or directory (os error 2)` |
| A field that isn't `title`, `root` or `watch` | `in <file>` — ``unknown field `colour`, expected one of `title`, `root`, `watch` at line 3 column 1`` |
| `title` or `root` missing | `in <file>` — `` missing field `title` `` |
| A field of the wrong type, e.g. `watch: "*.md"` | `in <file>` — `watch: invalid type: string "*.md", expected a sequence at line 3 column 8` |
| `root` isn't an existing file | `in <file>` — `root page <root> does not exist` |
| A `watch` pattern isn't a valid glob | `in <file>` — `watch pattern <pattern>`, then the glob error, e.g. `error parsing glob 'a[b': unclosed character class; missing ']'` |
| The folder can't be watched | the file-watcher's error |
| Every port from 3030 to 65535 is taken | `no free port` |

Unknown fields are rejected (`#[serde(deny_unknown_fields)]`), not ignored:
a misspelled field stops devpage with the error above.

## Example

devpage's own documentation, `docs/devpage.yml`:

```yaml
# devpage's own documentation, served by devpage.
title: devpage
root: README.md
watch:
  - "**/*.md"
```
