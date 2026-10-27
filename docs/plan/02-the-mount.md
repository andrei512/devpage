---
id: "02"
blocked-by: []
boxes: [viewer/serving-pages, viewer/page-script, viewer/widgets, viewer/designs, viewer/search, viewer/server, engineering/code-map]
---

# The mount: every address a page carries starts with its mount prefix

The prototype found that only redirects honour the resolver's mount. Make a
version served under `/d/<id>/v/<n>/` — and the latest under `/d/<id>/` — a
whole devpage, with no workaround.

## Build

- **`ServedFolder` carries its mount, and is passed, not rebuilt.**
  `Document::load`, the index, the entry's reload, linked documents in check,
  module snapshots and the design store take the served folder rather than a
  bare path, so the mount set once reaches everything loaded from it. Delete
  the prototype's `path → mount` registry.
- **Every address carries the mount** except the built-in assets', which are
  the same for every document: page links, sidebar, navbar, widget and helper
  addresses, and the page script's endpoints — events, comments, search,
  widgets, the design canvas and the design watch stream. Delete the
  prototype's referer routing.
- **The latest version is served in place at `/d/<id>/`** (the design's "the
  link always shows the latest"): the mount `/d/<id>` answers with the latest
  version; `/d/<id>/v/<n>/` with version `n`. `/d/<id>` redirects to
  `/d/<id>/`.
- Local `devpage <path>` and bundled mode are unchanged: their mount is empty.

Lane 01 is splitting `drive/server_mode.rs` into modules at the same time;
touch only its per-version routing (and delete the referer workaround there).

## Acceptance

- Chrome tests (`--ignored`) of a published version and of the latest:
  sidebar and navbar links, a project widget and a system widget, search,
  an embedded design drawing through its watch stream, and the event stream
  connecting — every request the page makes stays under its mount.
- Every existing test passes unchanged in meaning; no module outside
  `served.rs` builds a `ServedFolder` from a bare path except at the edges
  (the CLI, the server's entry).
- Code map and the serving-pages box updated.
