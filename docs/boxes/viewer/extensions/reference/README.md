# Package and host reference

This page specifies the local native extension package and the operations devpage exposes to it. It is the contract reference for [extensions](../README.md); the [local test](../local-test/README.md) exercises each capability with a consumer.

The [local runtime protocol](runtime.md) describes the implemented adapter and its limits. This reference defines the broader package contract.

## Configuration

`extensions` is an optional mapping on site, doc, and slides configurations. Each key is a package ID. Its value has exactly `package`, a path to a local bundle, and `config`, the extension-specific mapping (empty by default). Unknown host fields fail like other unknown document fields. Extension configuration errors retain the YAML path, instance ID, and the extension's explanation.

Paths in configuration are relative to the configuring YAML, not the process's current directory. An extension declares document inputs within the canonical served-folder boundary. A package itself is installed code and may live outside that content boundary; loading it does not make its directory publicly readable.

The initial commands are the existing serve, check, and PDF commands. There is no extension-specific command or automatic installation step. Check and PDF use the owning document's enabled packages when passed one of its Markdown pages. Within one document, enabling two providers for the same fence name, file-view suffix, or route namespace is an error, rather than an order-dependent override. Built-in route and language claims are reserved.

## Local bundle

A bundle contains `extension.yml`, a native library, declared syntax files, and optional browser assets. For example:

```yaml
id: egraph
version: "0.1.0"
api: 1
target: aarch64-apple-darwin
library: libegraph_devpage.dylib
assets: assets
```

The example is a macOS build; the library file and target describe the actual platform build. Manifest paths are relative to the bundle and cannot escape it. `assets` is optional. The SDK build produces the manifest and library; the package developer supplies the assets. The host checks the manifest version, package ID, API number, target, and library before creating an instance. Package content digests identify its immutable browser asset URLs and the implementation used to derive state. A semantic version alone is not a content identity.

The local `package` path refers to this built bundle. Changing the library or manifest requires a devpage restart. Edits to declared document inputs use live updates; they do not unload or rebuild native code. Assets are served only through the instance's namespaced asset addresses, never by exposing the package directory as another served folder.

## Rust SDK and native boundary

The host loads the C-compatible symbol `devpage_extension_v1`. It returns a function table with its API version and size, a dispatch function, and a release function. No Rust trait object, `String`, `Vec`, reference, or allocator-owned object is passed directly across the boundary.

Dispatch takes UTF-8 request bytes and returns response bytes; the API defines the request/response schema and operation names. JSON is their initial in-memory encoding, not a network protocol. The provider owns returned bytes until its release function is called; the caller owns request bytes for the duration of the call. Host and package must not free each other's allocations. The Rust SDK performs encoding, typed validation, allocation, and panic-to-error conversion for extension authors. Rust's native ABI offers no stability guarantees ([Rust reference](https://doc.rust-lang.org/reference/items/external-blocks.html#abi)); the SDK does not export Rust implementation types as a binary plugin contract.

The host checks the function-table header against the manifest before dispatching. It keeps the library loaded until instances and calls are finished. Calls for one instance are serialized; separate instances may run concurrently. Shutdown disposes instances before releasing the library. A native abort or invalid memory access can terminate the host: ordinary operation errors and caught Rust panics are reported, but process-level crash containment is not promised.

The core consumes a safe extension port. The binary implements that port with package discovery, native library loading, source capture, and asset reads. The SDK is a separate crate; it must not depend on a domain extension such as egraph.

## Operations and state

The API has these operations. The versioned SDK owns their typed request/response definitions; omitted optional capabilities are absent, not successful empty implementations.

| Operation | Input | Result |
| --- | --- | --- |
| Create | Document identity, configuration directory, served scope, extension config | Instance ID and capability/input declarations, or located configuration errors |
| Prepare | Captured input contents and identities, change/removal paths, capture problems | Published revision with diagnostics, or preparation failure |
| Augment page | Authored Markdown, its source identity/hash, render context | Anchored Markdown additions with origins and dependencies |
| Render embed | Resolved source target, label, render context | Embed contribution, assets and readiness obligations |
| Render file view | Served source target and render context | Full viewer contribution and its supported targets |
| Answer route | Read-only namespaced request and render context | Typed status, content type, response body and dependencies |
| Resolve reference | Owned source path/fragment or extension address, render context | Existing canonical target, missing target, or unsupported reference |
| Dispose | Instance ID | Release instance resources |

Every render context identifies the configuring document, mount prefix, bound extension revision, screen/print mode, theme, and source origin. Revision binding also applies when checking links or gathering content for search. Canonical URLs are constructed through the host's served-folder/mount helpers, rather than by concatenating machine paths.

Prepare receives supplied contents, not permission to read arbitrary files. An input declaration identifies a file or a bounded directory tree and include patterns. It covers creation and deletion, including paths that do not exist yet. The host applies path safety, ignore policy, and watch planning before filtering events. An extension's explicit inputs participate like explicit document dependencies; authors need not duplicate them in `watch`. The initial implementation accepts UTF-8 source text; no arbitrary storage backend is selected.

The extension may use its own embedded schema as code, identified by the package digest. A render-affecting document dependency must be declared and captured through the host. Native code can technically bypass that API, but doing so violates the extension's dependency contract; the API is not a native-code sandbox.

A successful preparation can contain domain diagnostics, as with an inspectable partial model. A failed preparation publishes no new successful revision. In local mode the previous revision may remain visible only with an explicit stale/error indication. Check exits unsuccessfully on a failed preparation; PDF cannot silently print an old revision as current. Source hashes supplied for a Markdown page must agree with the extension's bound capture when that page is one of its inputs; mismatches cause preparation or an explicit diagnostic, not unmarked mixed content.

## Rendering contributions

An embed is claimed by source format, following the existing `.design` image convention. `![System](../system/project.eg#root)` asks the model extension to render a selected view. A normal link to that target opens its full viewer. A fenced `eg` block remains highlighted source. Those meanings must not be conflated.

Page augmentation inserts Markdown at a known source heading or section boundary before the normal content-rendering pipeline completes. It does not replace the whole page or patch arbitrary HTML after the frame is built. The host preserves original source positions and distinguishes generated content origins, so comments on authored text do not attach to generated facts. Missing anchors and duplicate generated IDs are located problems.

A contribution supplies content, declared browser modules/styles, source origins, dependencies, diagnostics, and any readiness condition. It then passes through the same expanded-content, link/ID collection, preview/check, frame, and rendered-content search paths as the page. Added headings and prose participate in those existing mechanisms; no independent search index of all domain model objects is introduced here. Ordinary pages without additions render as before.

Browser modules and styles use package-digest asset URLs under the document's mount. Client behavior lives in those modules, not inline executable script injected into every page. Initial data is escaped as data. Embeds carry only their needed projection; opening the full viewer can obtain a complete coherent model separately. A request for a revision no longer available returns an explicit changed-state result; it cannot substitute new data beneath an old revision label.

A print render uses the same model revision and diagram projection as its content render. SVG embeds remain SVG, interaction chrome is hidden, and PDF waits for the declared readiness condition. The extension must report an unavailable print representation rather than omit a diagram silently. This extends the existing rule that a PDF is the page; it does not add a second diagram renderer.

## Languages and browser assets

Create may register syntax definitions with `fences` (language names such as `eg`), `file_extensions` (such as `eg`), and `grammar` (a bundle-relative syntect-compatible `.sublime-syntax` file). The host validates and loads those definitions once for the document's extension context and package digest. It emits the existing `hl-` scope classes and uses devpage theme tokens. An invalid registered grammar is an activation error; unknown, unregistered languages retain the ordinary escaped-code fallback. No native call per token is needed.

Browser modules and styles are declared bundle-relative assets. The host constructs their mounted, digest-qualified URLs. An extension's embed or workspace root receives its own identity so multiple embeds do not share element or SVG marker IDs. Package styles are scoped to those roots; adding a diagram must not restyle the surrounding Markdown or another package. Readiness is signaled for each contribution after its first stable render, and PDF consumes that same readiness result.

## File views, routes and references

Registered file views operate on existing served source paths after normal path validation. They do not introduce another top-level YAML `kind`. A model file's URL can open the viewer, with the fragment selecting its declared target. The first local workflow starts from an ordinary project document; direct CLI invocation on a custom file format is not implied by this registration.

Additional routes and assets live under `<mount>/__devpage/extensions/<owner>/<id>/`. The host supplies `owner`, an opaque key for the normalized document identity; it selects the enabled instance before dispatch. Two sites may enable the same package with different model configurations without sharing state or endpoints. The host reserves that namespace, handles URL normalization and GET/HEAD method policy, and dispatches only the suffix. Extensions cannot override another package or built-in route. A full viewer is a workspace response, not a diagram squeezed into the normal article column. Source and target selection remain discoverable through normal links.

The checker uses the same owned target index as the file view. A valid model fragment must select or inspect that element when followed, including an element not initially visible in the highest-level diagram. A missing fragment is a checked broken reference. This is a format-specific target contract, not a second guess at Markdown anchors; ordinary Markdown anchors still come from its rendered content.

The original source position of an authored link or embed accompanies an error. A generated link uses its declared origin. Raw file fallback does not make every custom-format fragment valid.

## Local and published modes

Local serve, check, and PDF resolve the same document activation and target meanings. The host uses its existing global refresh notification after preparing changed instances and before background reindexing. No page-targeted push protocol is added by this contract.

Publishing documents with enabled extensions fails explicitly until server-side package availability and captured dependency packaging are implemented. Document archives do not install or execute a native package on a remote server. Registry acquisition, automatic compilation, hot loading, and historical revision retention are separate work.
