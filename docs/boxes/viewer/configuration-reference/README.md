# Configuration reference

The exact contract of a devpage document file and of the `devpage` command
line: every field of each kind with its type, default and validation, the
commands and flags, and the errors devpage stops with. It is the reference page
under the [viewer](../README.md).

## Command line

```sh
devpage [path] [--root <folder>]
        [--host <address>] [--port <port>] [--user-header <name>] [--no-comments]
devpage check [path] [--root <folder>] [--json]
devpage pdf <path> [--root <folder>] [-o <file>]
devpage png <path> [--slide <n>] [--theme light|dark] [--root <folder>] [-o <file>] [--json]
devpage video <path> [--root <folder>] [-o <file>]
devpage server
devpage context add|use|list …
devpage publish <path> [--root <folder>] [--id <uuid>] [--new]
devpage list | versions | comments | download | delete …
devpage --help        # every command, and examples
devpage <command> --help
devpage --version
```

| Command | Does |
| --- | --- |
| `devpage [path]` | Serves the document at `path` and everything in the served folder, until stopped. |
| `devpage check [path]` | Checks the document at `path` and everything reachable from it by links; prints each problem and exits `1` if there was any, `0` if not ([Checking links](../checking-links/README.md)). `--json` prints them as JSON. |
| `devpage pdf <path>` | Prints a doc or a deck to PDF ([Printing to PDF](../pdf/README.md)). |
| `devpage png <path>` | Draws a design, or slide `--slide <n>` of a deck, to a PNG with no server running — the picture `<design>.png` or `<deck>/<n>.png` answers ([a design as a PNG](../designs/README.md#looking-at-a-design-as-a-png), [a slide as an image](../making-slides/README.md#a-slide-as-an-image)). A design with problems isn't drawn: its problems are printed and it exits `1`. A doc or a site is refused. |
| `devpage video <path>` | Records a deck to mp4, each slide's notes read aloud by GPT-Live ([Making a video](../making-a-video/README.md)). |
| `devpage server` | Serves published documents, configured only by `DEVPAGE_*` variables ([Server mode](../server/README.md)). |
| `devpage context`, `publish`, `list`, `versions`, `comments`, `download`, `delete` | Talk to a devpage server; every one is in the [server reference](../server/reference/README.md#cli). |

`path` is read the same way by every command:

| `path` | Means |
| --- | --- |
| a folder | the document file `devpage.yml` inside it |
| a `.md` file | that file, as a doc of one chapter with default settings |
| a `.design` file | that [design](../designs/README.md), titled by its file name without `.design`: `/` leads to its canvas, and `devpage check` reports its problems |
| any other file | a document file, whatever its name |
| none | `.` — `devpage.yml` in the current folder |

### Flags

| Flag | Command | Default | Meaning |
| --- | --- | --- | --- |
| `--root <folder>` | `devpage`, `check`, `pdf`, `png`, `video`, `publish` — the commands that read documents | the current folder | The [served folder](../writing-a-document/README.md#the-served-folder). The document must be inside it. |
| `--host <address>` | `devpage` | `127.0.0.1` | The address to listen on. Any non-loopback address makes devpage [deployed](../deploying/README.md). |
| `--port <port>` | `devpage` | the first free port from 3030 up | Listen on exactly this port; fail if it is taken. |
| `--user-header <name>` | `devpage` | `X-Forwarded-Email` | The request header naming a comment's author when deployed. Ignored locally. |
| `--no-comments` | `devpage` | comments on | Turn comments off: the pages show none and every comment request answers `404` ([Turning comments off](../comments/README.md#turning-comments-off)). |
| `-o, --output <file>` | `devpage pdf` | `<title>.pdf` in the current folder | Where to write the PDF. |
| `-o, --output <file>` | `devpage video` | `<title>.mp4` in the current folder | Where to write the video. |
| `-o, --output <file>` | `devpage png` | `<title>.png`, or `<title> <n>.png` for a slide, in the current folder | Where to write the PNG. |
| `--slide <n>` | `devpage png` | none | The slide to draw, from 1. A deck needs one; a design takes none. |
| `--theme light\|dark` | `devpage png` | `light` | The theme a design is drawn in. A slide takes none. |
| `--json` | `devpage check`, `devpage png`, `devpage context list`, the server commands | text | Answer in JSON, one line: the problems as `{"problems": [{"file", "line", "column", "message", "target"}]}` (a field with no value left out), `png`'s file as `{"png": "<file>"}`, and an error as `{"error", "causes"}` on standard error. |

### Exit codes

Every command exits with one of three codes, so a script or an agent can tell
a document's problems from a command that couldn't run:

| Code | Means |
| --- | --- |
| `0` | Done. |
| `1` | The document has problems — `check` found some, `png`'s design has some, or `publish` refused to send them — and they were printed. |
| `2` | devpage couldn't run: a command line it can't read, a document it can't load, a server that refused or isn't there. The error is on standard error. |

### Environment variables

`devpage video` reads the first four, and never `OPENAI_API_KEY`
([the narrator](../making-a-video/README.md#the-narrator)). `devpage server`
reads its own, listed in the
[server reference](../server/reference/README.md#environment), and the
server commands read `DEVPAGE_CONTEXT`. A variable set to an empty string
counts as not set.

| Variable | Default | Meaning |
| --- | --- | --- |
| `DEVPAGE_OPENAI_API_KEY` | none | Narrate with GPT-Live on OpenAI, with this key. |
| `DEVPAGE_AZURE_OPENAI_ENDPOINT` | none | Narrate with GPT-Live on this Azure Foundry resource — `https://<resource>.openai.azure.com/` or its bare host. Set, it selects Azure over OpenAI. |
| `DEVPAGE_AZURE_OPENAI_API_KEY` | none | The Azure resource's key; required with the endpoint. |
| `DEVPAGE_VIDEO_MODEL` | `gpt-live-1` | The model; on Azure, the deployment's name. |
| `DEVPAGE_CHROME_SANDBOX` | `on` | `off` runs Chrome — for `devpage pdf`, a slide's PNG, `devpage video` and the server's downloads — without its sandbox. Only for a container, which has no sandbox for Chrome to use and is the boundary itself; the [server image](../server/running-a-server/README.md) sets it. |

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
| `kind` | `site` | yes | Makes this file a site. |
| `title` | string | yes | The site's name: the sidebar heading and the second half of each tab title (`<page> · <title>`), or the whole tab title on a page titled like the site (trimmed, ignoring case). |
| `root` | path | yes | The page the site opens on and the top of its sidebar tree. |
| `watch` | list of glob patterns | no | Narrows which files in the site's folder refresh open pages to those matching a pattern, relative to that folder. It applies to this site's folder only (a nested site's own list applies inside it), whichever document devpage was started with; document files always refresh. A `.gitignore`d file never refreshes, and is watched — so the served folder and designs hear of it — only in an ignored folder a pattern explicitly reaches into: its part before the first glob character is in that folder or is it (`build/*.svg` reaches into an ignored `build/`; `**/*.md` reaches into none) ([the watch set](../live-updates/README.md#the-watch-set)). Default: every file that no `.gitignore` excludes. |
| `widgets` | path | no | The folder of this document's [project widgets](../widgets/project-widgets/README.md), relative to this file: every `.tsx` in it whose name starts with a capital letter is a widget. Default: none — only the system widgets. |
| `navbar` | map | no | The bar across the top: label → target links, dropdowns and icons ([the navbar](../organizing-navigation/README.md#the-navbar)). Default: no navbar. |
| `sidebar` | boolean | no | Whether the Topics sidebar shows. Default: `true`. |
| `theme-selector` | boolean | no | Whether the button that switches between light and dark shows ([choosing a theme](../themes/README.md#choosing-a-theme)). Default: `true`. |
| `logo` | path | no | An image — usually an SVG — shown before the title in the navbar (or the sidebar heading when there is no navbar) and used as the browser tab's icon. Default: none. |
| `extensions` | map | no | [Extensions](#extensions) the site enables: native packages, and [`code`](#code), its code reference. Default: none. |

A site's pages are the Markdown files in its document file's folder and below
it; a page there is shown in this site's frame. When sites are nested, a page
belongs to the nearest one. A `404.md` beside the document file is shown, in
the site's frame, at every address in its folder with nothing at it
([a page for missing addresses](../making-a-site/README.md#a-page-for-missing-addresses)).

### `doc`

| Field | Type | Required | Default | Meaning |
| --- | --- | --- | --- | --- |
| `kind` | `doc` | yes | | Makes this file a doc. |
| `title` | string | yes | | The doc's name: above the first chapter, in the tab, and the PDF's title and default file name. |
| `chapters` | list of paths | yes | | The chapter files, in reading order; at least one. |
| `pdf.paper` | `A4` or `Letter` | no | `A4` | The paper size. |
| `pdf.header` | string | no | none | Printed at the top of every page. |
| `pdf.footer` | string | no | `"{page} / {pages}"` | Printed at the bottom of every page. |
| `widgets` | path | no | none | The folder of this doc's [project widgets](../widgets/project-widgets/README.md). |
| `theme-selector` | boolean | no | `true` | Whether the light/dark switch shows. |
| `logo` | path | no | none | The browser tab's icon, and an image before the title. |
| `extensions` | map | no | none | Native [extension](#extensions) packages the doc enables. |

In `header` and `footer`, `{page}` is the page's number and `{pages}` the count.

A `.md` file given instead of a document file is a doc whose one chapter is that
file, titled by its first `# ` heading (or its file name), with every `pdf`
default.

### `slides`

| Field | Type | Required | Meaning |
| --- | --- | --- | --- |
| `kind` | `slides` | yes | Makes this file a deck. |
| `title` | string | yes | The deck's name: in the tab, and the PDF's title and default file name. |
| `slides` | list of paths | yes | The slide files, in order; at least one. |
| `audience` | string | for `video` | Who watches the talk; [told to the narrator](../making-a-video/README.md#what-the-narrator-is-told). `devpage video` refuses a deck without it. |
| `goal` | string | for `video` | What the talk should achieve; told to the narrator. `devpage video` refuses a deck without it. |
| `tone` | string | no | The energy of the whole talk; default "Natural and upbeat, like a speaker who enjoys this talk. Engaged and warm, never flat and never salesy." |
| `pronounce` | list of strings | no | One line per word a voice gets wrong, such as `Say "devpage" as "dev page".`; default none. |
| `widgets` | path | no | The folder of this deck's [project widgets](../widgets/project-widgets/README.md); default none. |
| `theme-selector` | boolean | no | Whether the light/dark switch shows; default `true`. Hidden in fullscreen. |
| `logo` | path | no | The browser tab's icon; default none. |
| `extensions` | map | no | Native [extension](#extensions) packages the deck enables; default none. |

### Extensions

A site, doc or deck accepts an optional `extensions` mapping. Each key is an extension ID; its entry names a built bundle with `package`, relative to this document file, and passes `config` to the package unread. The [package reference](../extensions/reference/README.md#configuration) gives the bundle's rules and errors. Enabling an extension does not compile or install it.

Extensions belong to the document that enables them. A nested site's pages use that site's configuration; a project-root site does not silently inject its extensions into another document. Printing one Markdown page uses the packages of its owning document.

### `code`

A site's `extensions` may hold `code` with no `package`: the
[system extension](../glossary/README.md#system-extension) that gives the site a
[code reference](../code-reference/README.md). Its `config` maps each
[namespace](../glossary/README.md#namespace)'s name — the first part of its
items' ids and addresses, `reference/<namespace>/` — to these fields:

| Field | Type | Required | Meaning |
| --- | --- | --- | --- |
| `files` | list of glob patterns | yes | The namespace's files, relative to the document file and inside the served folder. `.gitignore`d and hidden files are left out. |
| `language` | string | no | The language they are written in. devpage knows `rust`. Default: the namespace's name. |
| `query` | path | no | A tree-sitter query file read instead of the language's built-in one ([Reading one file](../code-reference/reading-the-code/README.md#reading-one-file)). Default: the built-in query. |
| `private` | boolean | no | Whether items no `pub` path reaches get pages too. Default: `false`. |

## Front matter

A page may open with YAML front matter: a `---` line first, the fields, and
another `---` line. It is never shown — the page renders from what follows —
and its title is still its first `# ` heading after it.

```markdown
---
search: true
description: How the outbox survives a restart.
keywords: [retry, offline]
---
# The outbox
```

| Field | Type | Meaning |
| --- | --- | --- |
| `search` | boolean | `false` leaves the page out of [search](../search/README.md#what-is-searched) though the served document reaches it; `true` puts it in though nothing reaches it. Default: searched when reached. |
| `description` | string | Words search finds the page by, weighted like its title. |
| `keywords` | string or list of strings | The same. |

Any other field is read by nothing and ignored.

## Errors

devpage stops before serving when the **entry** document can't be loaded. It
prints `Error:` and what it was doing — `reading <file>` or `in <file>` — then
the cause, on standard error, and exits `2`. With `--json` the same error is
one line of JSON instead: `{"error": "reading <file>", "causes": ["No such file or directory (os error 2)"]}`.

### Loading the document

| Problem | Message |
| --- | --- |
| A folder holds no `devpage.yml` | `<folder> has no devpage.yml` (`the current folder` for `.`), then a Markdown file to name instead — the folder's `README.md` when it has one — and where making a site is explained |
| The file can't be read | `reading <file>` — then the system error, e.g. `No such file or directory (os error 2)` |
| `kind` missing | `in <file>` — `` missing field `kind` `` |
| `kind` not one of the three | `in <file>` — ``kind: unknown variant `deck`, expected one of `site`, `doc`, `slides` at line 1 column 7`` |
| A field the kind doesn't have | `in <file>` — ``unknown field `root`, expected one of `title`, `chapters`, `pdf` `` |
| A `watch` pattern isn't a valid glob | `in <file>` — `watch pattern <pattern>`, then the glob error, e.g. `error parsing glob 'a[b': unclosed character class; missing ']'` |
| A required field missing, or of the wrong type | `in <file>` — the YAML error, with its line and column |
| `root`, a chapter or a slide isn't an existing file in the served folder | `in <file>` — `<field> <path> does not exist` |
| `chapters` or `slides` is an empty list | `in <file>` — `chapters lists no files` or `slides lists no files` |
| `widgets` names no folder, or one outside the served folder | `in <file>` — `widgets <folder> is not a folder` |
| A `navbar` dropdown holds another dropdown | `in <file>` — `navbar: <entry> under <dropdown> must be a link — dropdowns are one level deep` |
| A `navbar` dropdown lists no links | `in <file>` — `navbar: <dropdown> lists no links` |
| A `navbar` entry is neither a link nor a map of links — a number, or a list | `in <file>` — `navbar: <entry> must be a link or a map of links`, or the YAML error for a list |
| A `navbar` key has neither a label nor an `(icon)` | `in <file>` — `navbar has an entry with neither a label nor an icon` |
| `extensions: code` on a doc or slides | `in <file>` — `extensions: code: only a site has a code reference` |
| An extension other than `code` without `package` | `in <file>` — `extensions: <id>: package is required — <id> is not a system extension` |
| `extensions: code` with no namespace in its `config` | `in <file>` — `extensions: code: config lists no namespaces` |
| A `code` namespace's field the namespace doesn't have | `in <file>` — `extensions: code`, then ``unknown field `bogus`, expected one of `language`, `files`, `query`, `private` `` |
| A namespace's `files` is an empty list | `in <file>` — `extensions: code: <namespace>`, then `files lists no patterns` |
| A `files` pattern reaches outside the served folder | `in <file>` — `extensions: code: <namespace>`, then `files <pattern> is outside the served folder` |
| A `files` pattern isn't a valid glob | `in <file>` — `extensions: code: <namespace>`, then `files <pattern>` and the glob error |
| A `language` devpage doesn't know | `in <file>` — `extensions: code: <namespace>`, then `language <language> is not one devpage knows: rust` |
| `query` names no file | `in <file>` — `extensions: code: <namespace>`, then `query <path> does not exist` |
| `query` doesn't compile against the grammar | `in <file>` — `extensions: code: <namespace>`, then `query <file>` and `line <n>: <kind> error: <what>` |
| The document isn't inside the served folder | `<path> is outside the served folder <folder>` |

### Binding the address

| Problem | Message |
| --- | --- |
| `--port` is taken | `binding <address>:<port>` — the system error |
| Every port from 3030 up is taken | `no free port` |

### After it starts

Once devpage is serving, an edit that makes the entry's file invalid doesn't
stop it either: `/` and the site's pages show the problem until the file is
fixed, and the last valid version is kept meanwhile. The same problems in a
document file reached by a link don't stop devpage: they are [problems](../checking-links/README.md), shown on the
pages that link to it and reported by `devpage check`. `devpage pdf` also stops
with `pdf prints a doc or slides, not a site` when given a site, with the
error `downloading chrome-headless-shell <version> for <platform> from <address> into <cache folder>`
when the first download of [its Chrome](../pdf/README.md#the-chrome-it-drives) fails,
with `chrome-headless-shell <version> has no build for <os> on <arch>` on a
platform that has none, and with `pdf prints a doc or slides, not a design` when given a design.

`devpage png` stops with
``png draws a design or a slide of a deck; print a doc with `devpage pdf` `` when
given a doc or a site, `which slide? --slide <n>, from 1` for a deck without
`--slide`, `--slide is for a deck; a design is one picture`, and
`--theme is for a design; a slide is drawn as the deck is`. A slide that isn't
ready after 20 seconds is drawn as it was, with a warning on standard error.

`devpage video` stops, before it starts Chrome, with
`video needs GPT-Live: set DEVPAGE_OPENAI_API_KEY, or DEVPAGE_AZURE_OPENAI_ENDPOINT and DEVPAGE_AZURE_OPENAI_API_KEY`
when no narrator is set, `DEVPAGE_AZURE_OPENAI_ENDPOINT is set but DEVPAGE_AZURE_OPENAI_API_KEY is not`,
`DEVPAGE_AZURE_OPENAI_ENDPOINT is not an address: <endpoint>`,
`video records slides, not a site` (or `a doc`, `a design`), and
`every slide needs <Notes> to be narrated; these have none: <n> (<file>), …`.
Once running it stops with `downloading chrome-for-testing <version> for <platform> from <address> into <cache folder>`
when the first download of [its Chrome](../making-a-video/README.md#the-chrome-it-drives)
fails, `chrome-for-testing <version> has no build for <os> on <arch>` on a
platform that has none, `connecting to GPT-Live at <address>` when the session can't open (a
wrong key is `HTTP error: 401 Unauthorized`), with the error GPT-Live sends as
`GPT-Live: <message>`, and with
`the narrator said nothing for slide <n> within 60 seconds`. It writes no
file unless the whole deck was recorded.

A design's problems never stop devpage, even when the design is the entry:
its canvas shows them over the last valid drawing, and `devpage check` prints
each as `<file>:<line>:<column>: <message>` and exits `1`.

## Examples

devpage's own documentation, `docs/devpage.yml`:

```yaml
# devpage's own documentation, served by devpage.
kind: site
title: devpage
root: README.md
widgets: widgets
watch:
  - "**/*.md"
  - "**/*.yml"
```

[The feature catalog](../../../features/devpage.yml) shows the doc kind and
[the devpage talk](../../../talks/devpage/devpage.yml) the slides kind; each
`devpage.yml` is beside its chapters or slides.
[The checkout prototype](../../../designs/checkout/checkout.design) shows
the design kind, which has no document file
(`devpage docs/designs/checkout/checkout.design`).
