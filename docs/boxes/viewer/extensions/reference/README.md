# Package and protocol reference

The exact contract between devpage and an [extension](../README.md) package: the configuration entry, the bundle, the native boundary, and each JSON request devpage sends with the reply it expects. The definitions live in `core/src/extensions.rs` (binding and requests), `core/src/extensions/browser.rs` (browser modules), `src/adapters/extensions.rs` (manifest and library loading) and `extension-api/src/lib.rs` (the SDK).

## Configuration

`extensions` is an optional mapping on a site, doc or deck. Each key is the extension's ID; its value has only `package` and `config`:

| Field | Meaning |
| --- | --- |
| `package` | Path of the bundle folder, relative to the YAML file. Required, except for the system extension [`code`](../../code-reference/README.md). |
| `config` | Any YAML, passed to `create` as JSON. Null when left out. Devpage does not read it. |

An unknown field is an error. A Markdown page uses the extensions of the document that owns it; with no owner, the entry document's. A page printed on its own uses its site's. A binding is kept while the document's whole `extensions` mapping and served folder are unchanged; any change creates the document's bindings again.

## Bundle

A bundle folder holds `extension.yml` and the library it names:

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
| `target` | Must be the target triple devpage was built for. |
| `library` | Path inside the bundle. |
| `assets` | Optional folder inside the bundle. Checked to stay inside, but nothing is served from it. |

Unknown fields are errors. A loading failure is reported as `Extension <id>: <reason>`; a manifest ID that differs from the key fails the first `create`.

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

Sent once per binding when the document loads. The reply declares:

| Field | Meaning |
| --- | --- |
| `input` | Required. One file or folder, relative to the document's folder. It must exist and lie inside the served folder. |
| `input_extensions` | Required. File suffixes to capture under `input`. |
| `file_extensions` | Required. Suffixes the package shows and checks. ASCII letters and digits; not `md`, `yml`, `yaml` or `design`, and not claimed by another extension of the same document. |
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
- Any other suffix sends `route` with that suffix as `path`, without the query. `mime` defaults to `text/plain` and `status` to `200`.

Only GET and HEAD reach it. Every response has `Cache-Control: no-store`. An error answers 500 with its text.

## Browser modules

Modules are compiled by the same TSX compiler as [project widgets](../../widgets/project-widgets/README.md) and use devpage's React. A relative import must resolve, with an optional `.tsx`, `.ts` or `.js` suffix, to another declared module; module and style paths must be canonical and stay inside the package. A widget name starts with an uppercase letter, uses letters, digits and `_`, and cannot be a system widget's; a name another widget already uses is reported as a problem. Declared widgets are added to the document's widgets, and every declared stylesheet is linked on every page of the document. Devpage does not scope package CSS.
