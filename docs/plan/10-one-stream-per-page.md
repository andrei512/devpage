---
id: "10"
blocked-by: []
boxes: [viewer/design-engine, viewer/designs]
---

# A page watches all its designs through one stream

As the [design engine](../boxes/viewer/design-engine/README.md#the-watch-stream)
page now says: `Watch` takes the set of designs a page shows and every update
names its design; the page holds one stream and reopens it when the set
changes; a deleted design's last update ends only that design; an in-place
navigation that leaves no design closes the stream. Also: a page refetch keeps
an embed's canvas even while the embed shows problems (find the canvas by
selector, not by first child).

## Acceptance

- Chrome tests: a page with eight embedded designs paints all eight, and an
  edit to its Markdown still reaches it; a doc with eight embeds prints in
  about the time a doc with one does; navigating in place from a page with
  embeds to a page with none leaves no design stream open; an embed with
  problems survives a page refetch.
- A server test: one Watch call over two designs gets both scenes, a patch
  naming the edited one, and goes on after one of them is deleted.
