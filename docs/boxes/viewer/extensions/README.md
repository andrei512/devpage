# Extensions

Extensions add document knowledge and views to devpage while authors keep running the normal `devpage` command. A local package supplies Rust behavior, syntax definitions, and browser assets. Devpage owns configuration, source capture, watching, rendering, checking, and the live-update lifecycle. This box belongs to the [viewer](../README.md).

## Use one project workspace

A project's entry document can bring explanatory pages and engineering sources together without copying them. For example, a root `devpage.yml` can select `docs/README.md` as its landing page and enable a model extension:

```yaml
kind: site
title: Project
root: docs/README.md
extensions:
  egraph:
    package: ./extensions/egraph
    config:
      model: system/project.eg
```

Run it from that project with `devpage .`. `package` names a local extension bundle, not a crate devpage must compile. All configuration paths are relative to the YAML file. `config` belongs to the extension, which validates it; devpage interprets only the host fields. The package's manifest ID must match the mapping key.

A package developer builds the Rust crate into that bundle. Document authors neither assemble a custom executable nor rebuild devpage to enable a package. A missing or incompatible package is a located configuration error. The host does not install, download, or compile packages as a side effect of opening a document.

## One extension, several presentations

A model extension can provide all three experiences from the same captured state:

- A normal Markdown requirement or decision page, enriched with relevant context.
- An embedded diagram selected through an ordinary Markdown image reference, such as `![System](../system/project.eg#root)` from a page under `docs/`.
- A full viewer at the source file's URL, such as `/system/project.eg#root`.

The `.eg` code-fence language highlights source; it does not turn every source example into a diagram. Image references ask for an embed. Links ask for the file view. This follows the existing distinction between source and embedded `.design` views without adding another capitalized widget namespace.

The host's extension points are source inputs, content contributions, file views, namespaced read-only routes, language definitions, and reference resolution. Browser assets and print behavior travel with the contributions that need them. The [reference](reference/README.md) defines the contracts; the [local test](local-test/README.md) supplies their first consumer and acceptance path.

## From a save to a rendered view

An instance belongs to the document that enables it: a site, doc, or deck. Its package library can be shared, but its configuration and derived state are not global mutable settings.

1. The host loads the package, validates configuration, and asks it to declare inputs and capabilities.
2. Those inputs join the host's watch set before events are filtered. The host captures their contents and supplies them to the extension.
3. The extension prepares a revision of its derived state. A render uses one published revision for its contributions and referenced data.
4. A saved input is handled in the existing ordered change burst. Extension state is updated before a refresh is announced.
5. Existing open pages refresh through devpage's global refresh event. This design does not claim page-targeted event delivery.

A published revision is a consistent interpretation of captured bytes, not a filesystem transaction across an editor's separate writes. Each preparation reports its diagnostics. An inspectable model with issues can be published as such; a failed capture or preparation cannot silently masquerade as a current successful revision.

## The host stays general

Devpage knows input paths, revisions, rendered contributions, links, and asset addresses. It does not know model requirements, processes, telemetry, or evaluation semantics. Egraph supplies those meanings through a separate adapter crate. A generic fixture package must exercise the same interfaces without depending on egraph.

The native package boundary uses a versioned C-compatible entry point and owned byte buffers. A Rust SDK exposes typed operations to extension authors. There is no second server or IPC. Native packages are trusted code in the host process; this is not a sandbox or crash-isolation boundary. The API supplies no document-write or command-execution capability.

## Reach and remaining scope

The first implementation supports local serve, check, and PDF through the same extension context. A Markdown page printed on its own keeps the extensions of its owning document, just as it keeps its widgets. Existing documents with no `extensions` field retain their behavior.

Publishing an extension-enabled document is rejected explicitly until package availability and captured-input packaging are supported by server mode. Registry installation, automatic package builds, library hot reload, arbitrary YAML document kinds, model-node search independent of rendered pages, and comments on generated model shapes are outside this change. Existing comments on authored Markdown must keep their source associations.

Implementation reaches document configuration, the served-folder/watch boundary, the ordered change pipeline, content rendering, highlighting, request answers, link checking, rendered-page search, and PDF readiness. Package discovery and native loading belong to binary adapters; the core keeps its runtime-independent ports. The showcase deck must gain a working extension example when the feature is built.

## Topics

- [Package and host reference](reference/README.md) — Bundle format, lifecycle, contribution and resolution contracts.
- [Local acceptance test](local-test/README.md) — One normal command exercising the complete integration path.

### Constraints

- [Extensions use the host watch](constraints/host-watch.md) — inputs enter filtering and updates precede refresh.
- [A render uses one extension revision](constraints/one-revision.md) — source, derived content, and references cannot silently mix revisions.
- [The native boundary is versioned](constraints/native-boundary.md) — Rust implementation layouts and allocator ownership never cross an unspecified ABI.
