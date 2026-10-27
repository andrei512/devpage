---
id: "04"
blocked-by: ["01", "02"]
boxes: [viewer/server, viewer/live-updates, viewer/pdf]
---

# Live update on publish, the server's pages, downloads

## Build

- **Live update:** a publish announces on the change bus, scoped to the
  document; every page open at `/d/<id>/` swaps in the new version as a saved
  file does locally. Pages on `/d/<id>/v/<n>/` don't change.
- **Pages:** `/` says this is a devpage server and how to connect, listing no
  documents. The navbar's version menu lists the versions; an old version
  shows a banner linking to the latest.
- **Downloads:** `/d/<id>/v/<n>/download` — a PDF for docs and slides, a PNG
  for designs — rendered as `devpage pdf` prints, once per version, kept in the
  cache as an entry of that version; the navbar's download button.

## Acceptance

- Chrome tests: a page at `/d/<id>/` updates when version 2 is published; the
  version menu and banner; a PDF and a PNG download, the second request served
  from the cache.
