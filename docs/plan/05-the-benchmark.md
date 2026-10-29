---
id: "05"
blocked-by: []
boxes: [engineering/tests, engineering/sdlc, viewer/server]
---

# The benchmark, complete

Take the prototype's `devpage-bench` to [the server's benchmark](../boxes/engineering/tests/README.md#the-servers-benchmark):

- The envelope's corpus from a seed: 10,000 documents and 50,000 versions,
  docs, sites, slides and designs, sizes with the long tail, devpage's own
  `docs/` among them; published eight at a time, the publish rate reported.
- Every scenario — views and the burst, misses, writes, herd, open pages,
  soak, knee — open-loop, three runs each and the median, soak once.
- The report: each number beside its target and a baseline's, the stage that
  took the time from `/metrics`; the numbers as JSON for the next run.
- `bench.yml`, run by hand: the compose stack with RustFS, the image from the
  checkout held to 2 CPUs and 4 GB, the report on the summary page, the
  numbers as the run's artifact, main's last run downloaded as the baseline.
  The same binary runs from a laptop against any server.
- `deploy/server/kubernetes.yaml` sets the pod's requests and limits to 2 CPUs
  and 4 GB and leaves the metrics port out of the Service.

## Acceptance

- The same seed generates the same corpus, byte for byte, in a test.
- A test of the scheduler: a server that stalls is charged its stall — latency
  counts from each request's scheduled time.
- A short run against a local server finishes and writes a report with every
  scenario and every target.
- The image builds with `bench/` in the workspace.
- The tests, CI and running-a-server docs match.
