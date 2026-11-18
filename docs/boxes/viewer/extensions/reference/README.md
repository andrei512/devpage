---
page-type: reference
---
# Package and protocol reference

The exact contract between devpage and an [extension](../README.md) package: the configuration entry, the bundle, the native boundary, and each JSON request devpage sends with the reply it expects. The definitions live in `core/src/extensions.rs` (binding and requests), `core/src/extensions/browser.rs` (browser modules), `src/adapters/extensions.rs` (manifest and library loading), `src/adapters/processes.rs` (a package's program), `src/drive/http/live.rs` (requests passed to it) and `extension-api/src/lib.rs` (the SDK).

## Configuration

`extensions` is an optional mapping on a site, doc or deck. Each key is the extension's ID; its value has only `package` and `config`:

| Field | Meaning |
| --- | --- |
| `package` | Path of the bundle folder, relative to the YAML file. Required, except for a [system extension](../../glossary/README.md#system-extension): `code`, `requirements` and `live`. |
| `config` | Any YAML, passed to `create` as JSON. Null when left out. Devpage does not read it. |

An unknown field is an error. A Markdown page uses the extensions of the document that owns it; with no owner, the entry document's. A page printed on its own uses its site's. A binding is kept while the document's whole `extensions` mapping and served folder are unchanged; any change creates the document's bindings again.

## Bundle

A bundle folder holds `extension.yml` and the library it names, the program it runs, or both:

```yaml
id: toy
version: "0.1.0"
api: 1
target: aarch64-apple-darwin
library: libtoy.dylib
```

| Field | Rule |
| --- | --- |
| `id` | ASCII letters, digits and `-`; must equal the configuration key. |
| `version` | Not empty. Otherwise unused. |
| `api` | Must be `1`. |
| `target` | With a `library`: must be the target triple devpage was built for. |
| `library` | Optional. Path inside the bundle. |
| `run` | Optional. The [program](#process) and its arguments, as a list; not empty. |
| `assets` | Optional folder inside the bundle. Checked to stay inside, but nothing is served from it. |
| `widgets` | Optional, for a package with no `library`. Widget name → the path of its module inside the bundle, as a library declares them in [`create`](#create). |

Unknown fields are errors, and so is a manifest with neither `library` nor `run`: `Extension has neither a library nor a program to run`. A loading failure is reported as `Extension <id>: <reason>`; a manifest ID that differs from the key fails the first `create`.

## Native boundary

The library exports `devpage_extension_v1`, which returns a pointer to a `VTable`: `version`, `size`, `invoke` and `release`. Devpage rejects the library unless `version` is `1` and `size` equals its own `VTable` size. `invoke` takes request bytes and returns a `Buffer` (`ptr`, `len`, `capacity`) holding the reply; devpage reads it and passes it back to `release`, so each side frees only its own allocations. A null reply is an error. The library is closed once no binding holds it.

Requests and replies are UTF-8 JSON. A reply is `{"ok": true, "value": …}` or `{"ok": false, "error": "…"}`; devpage reports the error text.

The `devpage-extension-api` crate implements this. A type that implements `Extension` (`Default`, plus `invoke(&mut self, Value) -> Result<Value, String>`) is exported with `export_extension!(Type)`. The macro keeps one instance per library behind a mutex, created on the first request, and turns a panic into the error `Extension panicked`. Two documents enabling the same bundle share that instance; each request's `instance` says which binding is asking.

## Requests

Every request has `op` and `instance`, the binding's ID: the first 16 hex digits of the SHA-256 of the document path, the extension ID and the document's `extensions` mapping. All but `create` and `prepare` also carry `base`, the binding's address, and `revision`, the revision prepared last.

| `op` | Also sent | Reply |
| --- | --- | --- |
| `create` | `id`, `config` | The [declaration](#create). |
| `prepare` | `sources` | `revision`: a non-empty string. |
| `augment` | `path`, `source` | `source`: the page's Markdown. Left unchanged when missing. |
| `embed` | `page`, `target`, `label`, `identity` | `html` |
| `view` | `path` | `layout`, `title`, `content` |
| `targets` | `path` | `ids`: list of strings |
| `route` | `path` | `body`, `mime`, `status` |

Paths are relative to the served folder, with `/`. An unknown `op` should be an error; devpage sends no others.

### Create

Sent once per binding when the document loads. A package without a library gets no requests: it declares only the `widgets` its manifest names, each with its module. The reply declares:

| Field | Meaning |
| --- | --- |
| `input` | Required when `input_extensions` or `file_extensions` names a suffix. One file or folder, relative to the document's folder. It must exist and lie inside the served folder. |
| `input_extensions` | Optional. File suffixes to capture under `input`. |
| `file_extensions` | Optional. Suffixes the package shows and checks. ASCII letters and digits; not `md`, `yml`, `yaml` or `design`, and not claimed by another extension of the same document. |
| `grammar` | Optional. Path of a Syntect `.sublime-syntax` file inside the bundle. Devpage reads it and validates it. Its own `file_extensions` are the fence names it highlights, and must not name a language devpage already highlights. |
| `modules` | Optional. Module path → TypeScript, TSX or JavaScript source. |
| `widgets` | Optional. Widget name → module path whose default export is the widget. |
| `styles` | Optional. Stylesheet path → CSS source. |

Any violation makes the document invalid.

### Capture and prepare

Devpage reads `input` recursively: names starting with `.` are skipped, a symlink or a path outside the served folder fails the capture, and only files with an `input_extensions` suffix are read, as UTF-8. `sources` maps each file's served-folder path to its contents, sorted.

Preparation runs once on load and again whenever a change under `input` arrives. It is skipped when the captured contents equal the last prepared ones. A failed capture or a failed `prepare` is kept: until a preparation succeeds, every other request fails with `Extension preparation failed: <reason>`. A reply without `revision` fails only that preparation.

### Pages and embeds

`augment` is sent for each page rendered for the document, before widget tags are expanded and Markdown is rendered, so what it adds is highlighted, gets heading IDs, and is link-checked and searched. With several extensions, each receives the previous one's output.

After rendering, each `src` attribute whose path ends in an owned suffix is an embed target. `embed` receives the page's path, the attribute's value as `target` (fragment included), an empty `label`, and `identity`, the target's position among the page's `src` values. The returned HTML replaces the `<img>` element. A print render sends the same request.

A failed `augment` or `embed` leaves the page as it was and adds a problem shown on the page and reported by `devpage check`.

### File views

A served file is owned when its suffix is in `file_extensions` and it lies under `input`. Opening it sends `view`. The reply must have `layout: workspace`; devpage puts `content` (HTML, which may contain widget tags) inside a full-width page with the document's theme, devpage's stylesheet and page script, and the document's widgets, and `title` in the tab. Any other layout is an error.

### Checked targets

A link with a fragment to an owned file sends `targets`; a fragment not in `ids` is a broken link. Links without a fragment are checked as ordinary files.

### Addresses and routes

A binding's `base` is `<mount>/__devpage/extensions/<instance>/<id>/`. Under it:

- `browser/<digest>/modules/<path>` serves a compiled module, and `browser/<digest>/styles/<path>` a stylesheet. `<digest>` is the SHA-256 of devpage's build and the declared modules, widgets and styles.
- `live/<rest>` is passed to the package's [process](#process).
- `pages/<page>` is a [page](#pages) the process writes.
- Any other suffix sends `route` with that suffix as `path`, without the query. `mime` defaults to `text/plain` and `status` to `200`.

Only GET and HEAD reach `route`. Every response has `Cache-Control: no-store`. An error answers 500 with its text.

## Process

The program a manifest's `run` names. It is started in the bundle folder by the first request under `live/` or `pages/`, in a process group of its own, with:

| Variable | Value |
| --- | --- |
| `DEVPAGE_PORT` | The loopback port to listen on, for HTTP. |
| `DEVPAGE_TOKEN` | The token every request from devpage carries. |
| `DEVPAGE_URL` | Devpage's own address, as a browser opens it. |
| `DEVPAGE_CONFIG` | The extension's `config`, as JSON; `null` when left out. |
| `DEVPAGE_ROOT` | The folder of the document that enabled it; for [the live session](../../live-session/README.md), the served folder. |

A `run` whose program is `bun` is run with the bun [`devpage setup`](../../configuration-reference/README.md#devpage-setup) installed, when there is one, and else with the `bun` on the `PATH`.

It has 20 seconds to accept a connection on the port. A request under `live/<rest>` — any method, its query, headers and body — is sent to it as a request for `/<rest>`, with two headers set by devpage, whatever the browser sent:

| Header | Value |
| --- | --- |
| `X-Devpage-Token` | `DEVPAGE_TOKEN`. A request without it did not come through devpage; the program should refuse it. |
| `X-Devpage-User` | Who is at the browser: the local git name. |

Its response is returned as it is. When it answers `101 Switching Protocols`, the two connections are joined until either closes: a WebSocket opened under `live/` is one to the program.

| When | Answer |
| --- | --- |
| Devpage serves on a non-loopback address | 404 `extension processes run only under `devpage` on a loopback address` |
| The request's `Host` is not `localhost`, `127.0.0.1` or `[::1]` | 403 `an extension's process answers only on localhost` |
| Its `Origin` is another one, or its `Sec-Fetch-Site` is neither `same-origin` nor `none` | 403 `an extension's process answers only devpage's own pages` |
| No extension is bound at the address | 404 `no extension is bound at this address` |
| The package has no `run` | 404 `extension <id> has no process` |
| The program ended before listening, or didn't listen in time | 502 with the reason |
| The program died | 502 `extension <id>: its process died`, then the end of what it wrote |

A program that isn't running is started by the next request; one that failed to start is tried again after 2 seconds, then 4, doubling to 30, and from the start once it has run for 10. It is stopped — asked with `SIGTERM`, with what it started, and killed after 3 seconds — when devpage exits and when its extension is no longer bound, which a change to the document's `extensions` mapping causes.

### Pages

A GET of `pages/<page>` under the extension's address asks the process for `/pages/<page>`, as a request under `live/` is passed on: the path as the browser wrote it, the query, and the same two headers. The answer's body is taken as Markdown and drawn as a page:

- In the frame of the document that binds the extension — the entry document when it does, else the first that does. A site draws it as one of its pages, with its navbar and sidebar; any other document as a doc of that one chapter, as a Markdown file served alone is drawn.
- Its widgets and diagrams are drawn as in any page. A relative link in it is resolved against the page's own address.
- Its content is named `<page>`: that is the `data-source` of its `<article>` or its chapter's `<section>`, so [a part](../../on-the-page/widgets/page-module/README.md#a-part-of-a-page) of it names its page as the extension does.
- It shows no comments and lists no problems, and `devpage check` does not read it: it is no file.

| The process answers | devpage answers |
| --- | --- |
| 2xx | 200, the page. A body over 8 MB is a 502. |
| 404 | 404, devpage's own page for an address with nothing at it |
| Anything else | 502 `extension <id>: its process answered <status> for pages/<page>`, then what it said |

The refusals of a request under `live/` hold here too: no process on a non-loopback address, only `localhost`, only devpage's own pages. An address under no bound extension's `pages/` is not found.

## Browser modules

Modules are compiled by the same TSX compiler as [project widgets](../../on-the-page/widgets/project-widgets/README.md) and use devpage's React. A relative import must resolve, with an optional `.tsx`, `.ts` or `.js` suffix, to another declared module; module and style paths must be canonical and stay inside the package. A widget name starts with an uppercase letter, uses letters, digits and `_`, and cannot be a system widget's; a name another widget already uses is reported as a problem. Declared widgets are added to the document's widgets, and every declared stylesheet is linked on every page of the document. Devpage does not scope package CSS.
