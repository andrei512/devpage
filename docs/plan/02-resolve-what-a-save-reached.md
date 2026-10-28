---
id: "02"
blocked-by: ["01"]
boxes: [viewer/design-engine]
---

# The store resolves again only what a save reached

The store finds each patch from what changed, never by comparing scenes.

Build:

- Per design, each node's provenance: its element and its instance's props;
  per file, the nodes made from it; per themes.yml / font / image, the nodes
  that read it.
- A changed design file: parse it against its last parse (01's matcher),
  resolve again the changed elements' nodes in their props, inserted elements
  as new subtrees, instances whose own attributes changed with their subtree;
  removed elements remove their nodes. A themes.yml/font/image change: the
  nodes that read it. The keyed patch is what was resolved again.
- Problems: while a design has problems, keep what each save reached; the save
  that makes it valid resolves it all against the last scene sent.
- One patch per change, made in the store and sent by every stream that sent
  the scene it starts from; a stream behind gets the whole scene.
- Keep `devpage check` (from disk) as is.

Acceptance:

- For every edit kind of 01's random test, made as file saves, the scene the
  store keeps equals a fresh resolve of the files, and the patch it sends
  brings the browser's scene to it.
- An edit to one row's text in a 1,000 / 10,000 / 100,000-row design: save to
  patch cost flat apart from parsing the saved file (measured and reported
  before/after); a component file edit touches only its instances.
- Problems then a fix; a deleted and rewritten design; two streams joining at
  different times — all still behave as the watch-stream docs say.
