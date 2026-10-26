# Serving pages

How the viewer answers a request: the URL names a file in the served folder,
the path is checked to stay inside it, and the file is served as what it is — a
document, a page in its document's frame, or bytes. It also covers the built-in
assets, the address devpage listens on and what it logs. It is a mechanism page
under the [viewer](../README.md), for someone about to change how requests are
served; [which document a file belongs to](../document-index/README.md) and
[rendering a page](../rendering-a-page/README.md) go one level deeper.

```mermaid
flowchart LR
  req["GET /path"] --> route{"Route"}
  route -->|"/"| entry["The entry document"]
  route -->|"/__devpage/…"| own["devpage's own:<br/>events, comments,<br/>widgets, assets"]
  route -->|"Watch, *.design.png"| designs["Designs:<br/>watch stream, PNG"]
  route -->|"anything else"| file["A file in the<br/>served folder"]
  click route href "#routes" "Routes"
  click entry href "#routes" "Routes"
  click own href "#routes" "Routes"
  click designs href "../design-engine/README.md#in-the-server" "The design engine in the server"
  click file href "#the-url-is-the-file-path" "The URL is the file path"
```

## Routes

| Route | Response |
| --- | --- |
| `/` | `307 Temporary Redirect` to the entry document as its file says now — a site's root page, or a doc's or deck's document file (or the Markdown file devpage was given); while the entry's file is invalid, the page saying why |
| `/__devpage/events` | the live-update event stream — see [Live updates](../live-updates/README.md) |
| `/__devpage/comments…` | the comment store's requests — see [Comments](../comments/the-comment-store/README.md#the-requests); `404` with comments off |
| `/__devpage/system-widgets/{name}` | a system widget, compiled — see [Widgets](../widgets/how-widgets-run/README.md) |
| `/__devpage/widgets/{*path}` | a project widget or helper in the served folder, compiled — see [Widgets](../widgets/how-widgets-run/README.md) |
| `/__devpage/{asset}` | one of the built-in assets |
| `/devpage.design.v1.Designs/Watch` | a page's watch stream over every design it shows, as gRPC-Web; it is the service's only method, and the only path under `/devpage.design.v1.Designs/` that answers — see [Design engine](../design-engine/README.md#the-watch-stream) |
| `/{*path}.design.png` | the design at `path` painted as a PNG — see [Looking at a design as a PNG](../designs/README.md#looking-at-a-design-as-a-png) |
| `/{*path}.yml/{n}.png` | slide `n` of the deck at `path.yml` (or `.yaml`) as a 1920 × 1080 PNG, drawn by chrome-headless-shell, with an `ETag` (`304` when the request holds it); `404` with the reason when the file isn't a deck or has no slide `n`; `508 Loop Detected` when a page devpage's own Chrome is shooting asks for a slide not drawn yet — see [A slide as an image](../making-slides/README.md#a-slide-as-an-image) |
| `/{*path}` | the file at `path` in the served folder |

A query of `?print` renders a doc's or deck's document file in **print mode**,
the page `devpage pdf` has Chrome print ([Printing to PDF](../pdf/README.md)).
A site is never printed, so on a Markdown file `?print` always renders it as a
doc of one chapter in print mode, even when the file is a page of a site — which
is how `devpage pdf` prints one Markdown file.

## The URL is the file path

A URL path is joined to the [served folder](../writing-a-document/README.md#the-served-folder)
— `/docs/boxes/README.md` is `<served folder>/docs/boxes/README.md` — so relative
links between files work as written. Nothing is precomputed: every request
reads its files from disk.

**Path safety.** Every path takes the same three checks
([the rule](../constraints/the-served-folder-is-the-boundary.md)):

```mermaid
flowchart LR
  path["/path"] --> join["Joined to the served folder,<br/>canonicalized"]
  join --> inside{"Still inside the<br/>served folder?"}
  inside -->|"yes"| file{"A regular file?"}
  inside -->|"no"| nf["404"]
  file -->|"yes"| ok["Served"]
  file -->|"no"| nf
  click ok href "#what-a-file-is-served-as" "What a file is served as"
```

Canonicalizing resolves `..` and symbolic links, and fails if the file doesn't
exist. Anything that fails — a path escaping with `..`, a symbolic link
pointing outside, a missing file, a folder — gets `404` with the body
`<path> is not part of the served folder`. A file that exists but can't be read
gets `500` with the error's text.

## What a file is served as

A file that passes the checks is one of four things:

| The file | Is served as |
| --- | --- |
| A document file — YAML with `kind: site`, `doc` or `slides` | the document, as its kind |
| A Markdown file | a page, in the frame of the document it belongs to |
| A `.design` file | the [canvas page](#the-canvas-page) |
| Anything else | its bytes, as they are — see [Other files](#other-files) |

### The canvas page

A `.design` file is shown on the canvas page: a frame holding a `<canvas>`,
the light/dark switch in the corner with the head script that sets the theme
before the page paints, and the design script, which draws it as the
[design engine](../design-engine/README.md#in-the-browser) describes and runs
the switch. It belongs to no site and has no sidebar, navbar or comments.

## Which document a file belongs to

What makes a file a document file, which site a page belongs to and the index
that answers it are on [Which document a file belongs to](../document-index/README.md).

## Rendering a Markdown file

How a page is turned into HTML and put in its frame is on
[Rendering a page](../rendering-a-page/README.md).

## Other files

Any other file is read as bytes and returned with a `Content-Type` guessed from
its extension (`mime_guess`), or `application/octet-stream` when the extension
is unknown.

## Built-in assets

The page's own files are compiled into the binary with `include_str!`, and
the design engine's module with `include_bytes!`:

| URL | Source | Content type |
| --- | --- | --- |
| `/__devpage/style.css` | `assets/style.css` | `text/css` |
| `/__devpage/devpage.js` | `assets/devpage.js` | `text/javascript` |
| `/__devpage/pele.js` | `assets/vendor/pele.js` | `text/javascript` |
| `/__devpage/react.js` | `assets/vendor/react.js` | `text/javascript` |
| `/__devpage/design.js` | `assets/design.js`, the design script: the canvas page's, and loaded by the page script on a page that embeds a design | `text/javascript` |
| `/__devpage/design-engine.js` | `assets/vendor/design-engine.js`, the [design engine](../design-engine/README.md#one-crate-two-targets)'s vendored bindings | `text/javascript` |
| `/__devpage/design-engine.wasm` | `assets/vendor/design-engine.wasm`, the design engine's vendored WebAssembly module | `application/wasm` |

They are sent with `Cache-Control: no-cache`; any other name under
`/__devpage/` is `404`. Changing an asset means rebuilding devpage.

## The address

devpage listens on `127.0.0.1` — the local machine only — unless `--host`
names another address, which makes it
[deployed](../deploying/README.md). It takes the first free port from 3030 up,
or exactly the port `--port` names. It prints `<title> → http://localhost:<port>`
(with the host in place of `localhost` when one is given) on standard output.

## Request logging

The server `devpage` runs has a middleware that logs every request to
standard error, as
`<method> <path> → <status> in <ms> ms`, for example
`GET /README.md → 200 in 0.7 ms`. The time is until the response starts, so
an event stream logs once, when it opens.

The exact document file fields and command line are in the
[configuration reference](../configuration-reference/README.md).

## Topics

- [Which document a file belongs to](../document-index/README.md) — document
  files, the nearest site, and the index that finds it without scanning.
- [Rendering a page](../rendering-a-page/README.md) — Markdown to HTML,
  highlighted code, the title, the sidebar and the page frame.
