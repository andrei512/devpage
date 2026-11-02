# Test a local extension

This acceptance path tests [extensions](../README.md) by running an ordinary project document through the installed `devpage` command. It covers a generic package first and the egraph consumer, without publishing, a registry, a custom host executable, or a second application server.

## Assemble the local inputs

The package developer builds a Rust extension bundle against API 1. The first host tests use a generic fixture package: it declares one text input, enriches a page, contributes an embed/file view and namespaced read-only endpoint, and supplies a small syntax definition. It depends on the SDK only. Its purpose is to make egraph-specific assumptions in the host fail visibly. It ships with the host implementation so an extension hook never arrives without a consumer.

The egraph adapter is a separate Rust crate owned by the egraph project. It uses the SDK and the existing egraph reader, plus its viewer assets. Viewer links, assets, and data requests take their base addresses from the host context; hardcoded standalone-server URLs do not pass the mounted test. It interprets host-captured sources with `model_from_sources`; it does not start egraph's server or a private watcher. Its bundle has the same manifest and lifecycle as the generic fixture.

Move the egraph project's existing site configuration from `docs/devpage.yml` to a project-root `devpage.yml`, adjusting its root, widget, and navigation paths to keep their destinations. It selects `docs/README.md`, enables the local package, and sets `config.model` to `system/project.eg`. Engineering sources stay in their existing nested subject folders. Do not leave an unintended nested `docs/devpage.yml` in place: nearest-site ownership would otherwise make the documentation pages belong to that unextended site. Other deliberately separate nested documents must enable their own packages explicitly. Update the project's run instructions to use `devpage .`.

## Run the normal command

```sh
devpage . --port 3042
```

The runtime must load the built package through configuration. Compiling the adapter directly into the host, starting another viewer server, or simulating the extension with an iframe does not pass this package-loading test. The host implementation is installed once before this acceptance run; extension authors do not assemble a custom executable.

## Follow one engineering change

1. Open a requirement page from `system/`. Its authored text and IDs remain ordinary Markdown. Extension context links the requirement to a specific design target and verification method; an unrelated requirement is not attached through a broad shared section.
2. Follow the design link to its `.eg` source URL. It opens the full viewer at the selected target. Out-zoom and in-zoom follow declared model facts, with a permanent sidebar and stable canvas geometry.
3. Open a guide containing both an ordinary Markdown image reference to a model view and a fenced `eg` example. The former is a diagram; the latter is highlighted source using devpage theme scopes. Opening the embed in the full viewer retains its selected target. Add a second embed and require independent element/marker identities, no cross-selection, and unchanged surrounding page styles.
4. Edit one model explanation or declaration. The host's existing watch receives the change, supplies a new captured revision, and refreshes the page and tool coherently. No extension-owned watch, polling loop, or host restart is needed for this source change.
5. Add a file in a newly created nested model folder, then delete it. Capture and diagnostics reflect both changes. Narrowing the site's `watch` list does not silently suppress a declared extension input.
6. Introduce a missing reference. The readable partial model retains its useful information and located issue; it is not presented as fully checked. Make source capture unavailable and require a visible failure/stale state rather than an old model labeled current.
7. Run `devpage check .`. Generated and authored links into registered model targets are checked through the same provider. A real target passes; a missing fragment reports the originating page and location.
8. Print the Markdown guide with `devpage pdf docs/extension-example.md -o /tmp/devpage-extension-example.pdf`. Its owning configuration remains active; the same diagram appears as SVG without viewer controls, source remains highlighted, and capture/readiness failures are reported.

The path proves package loading, watcher integration, rendering, highlighting, navigation and checking together. It does not prove a general language checker, requirement satisfaction, or improved engineering productivity.

## Test the boundary failures

Exercise a missing library, wrong target or API, mismatched package ID, invalid extension options, duplicate format ownership, and a library missing its entry point. Each gives a located error; ordinary documents with no enabled extension remain usable.

Reject document inputs escaping the served scope, package assets escaping the bundle, and attempts to take a built-in route. Exercise a non-empty mount so routes, assets, redirects, model links and API requests retain their prefix. Verify unsupported methods do not reach the extension route handler. Enable the same package in two deliberate document owners with different inputs and require separate instances and namespaced data; neither may answer from the other’s model.

Keep comments on authored paragraphs attached after content additions. Check that rendered headings and links are the ones used by checking and rendered-content search. Source edits during rendering must produce one bound revision or an explicit inconsistency result. Dispose instances before unloading libraries, with no returned buffers used after release.

## Prototype result

The spike reports which host contracts actually moved, where the native SDK or rendering seams were costly, contradictions found, and the cases still untested. It records commands and observations; it does not turn this acceptance list into a claim that the integration already works. Implementation tickets follow that measured result.
