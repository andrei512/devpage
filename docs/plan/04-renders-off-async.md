---
id: "04"
blocked-by: []
boxes: [viewer/serving-pages]
---

# Pages resolved on blocking threads

Resolving a path — reading the file, checking it, rendering it in its frame —
runs on a blocking thread, in a local `devpage` and in server mode alike, so a
slow page never holds up a cheap page, an asset, the live-update stream or a
publish ([serving pages](../boxes/viewer/serving-pages/README.md#rendering-a-markdown-file)).

## Acceptance

- A test holds several slow page resolutions at once and a built-in asset and
  a cheap page still answer at their usual speed.
- Every page answers as before: the route tests and the Chrome tests pass
  unchanged.
