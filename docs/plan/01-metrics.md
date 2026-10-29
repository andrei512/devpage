---
id: "01"
blocked-by: []
boxes: [viewer/server]
---

# The metrics, complete

The prototype's metrics port serves `check`, `nav`, `render`, `store` and
`router`. Finish it to what [measuring a server](../boxes/viewer/server/measuring-a-server/README.md#metrics)
describes:

- Every stage in the design's table — `fetch`, `unpack`, `publish-check`,
  `upload` and `download` too — timed where that work happens.
- `render` timed at the one place every kind of page is rendered, so a doc's
  chapter counts as a site's page does.
- Requests by route — as the router declares it — and status, with their time.
- The totals: versions held in memory and their routers, threads, open live
  pages, the cache's bytes, hits, misses and evictions, resident memory.

## Acceptance

- A test asks `/metrics` on the metrics port and finds each stage, the request
  series and each total; a test asks `/metrics` on `DEVPAGE_PORT` and gets no
  metrics.
- A config test covers `DEVPAGE_METRICS_PORT`: its default, a value, a
  malformed value refused by name.
- A test publishes and views documents with distinct ids and finds no id,
  version or page in any label ([metrics name no document](../boxes/viewer/server/constraints/metrics-name-no-document.md)).
- The reference, configuration and tests docs match.
