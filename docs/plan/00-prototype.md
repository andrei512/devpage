# Prototype: measuring a server

Spike of `design/server-benchmark`, 09:32 → 09:45 by the clock (13 minutes of
the 30). One end-to-end path runs: `devpage-bench` generates a seeded corpus,
publishes it through the API, sends page views at scheduled times, scrapes
`/metrics` on the metrics port before and after, and prints the report.

## The first numbers

The image built from this branch, held to the envelope's pod (`docker run
--cpus=2 --memory=4g`), `file://` bucket, 300 generated documents per run
(sites and docs of linked pages, half the links to a `#heading`), every page
read once before the views; one run each, 15 s of views:

| views asked | answered/s | p50 | p99 | target p99 100 ms |
| --- | --- | --- | --- | --- |
| 50/s | 50 | 5.3 ms | 53.7 ms | met |
| 200/s | 172 | 643 ms | 3,152 ms | missed |
| 500/s (the burst) | 396 | 1,632 ms | 4,985 ms | missed |

Time per stage at 200/s, from `/metrics`:

| stage | times | mean | total |
| --- | --- | --- | --- |
| check | 3,000 | 10.0 ms | 30.0 s |
| render | 1,654 | 0.1 ms | 0.18 s |
| nav | 1,654 | 0.0 ms | 0.03 s |
| store | 6,000 | 0.0 ms | 0.05 s |

30 s of check in 15 s of run is both CPUs: **the link check is the knee**,
between 50 and 200 views a second on 2 CPUs. Render, nav and the store are
noise beside it on this corpus. After the three runs — 900 documents read —
the pod held **3.5 of its 4 GB and 5,404 threads**: the per-version search
writer the probe found, still the first thing that kills the pod.

Publishing, eight at a time: 93–97 a second, p99 87–133 ms, for these small
documents (a few KB each, not devpage's 425 KB site).

On the laptop's 18 cores (release build, no limit) the same check took 4–12 ms
a view and one 50/s run missed at p99 154 ms while runs at 200/s and 500/s met
— run-to-run noise that large on a quiet machine is why the design's three
runs and median are needed.

## What it touched

- **The core** (`check.rs`, `nav.rs`, `render/frame.rs`): one `tracing` span
  each — `check` around `Preview::problems`, `nav` around `nav::tree`,
  `render` around `render::page`. `tracing` added to the core's dependencies;
  the core's rule against I/O runtimes is untouched.
- **Server mode**: `metrics.rs` — a closed `Stage` set, histograms on atomics
  with fixed buckets, the Prometheus text by hand, and `Stages`, a minimal
  `tracing::Subscriber` timing each stage span from made to closed. No
  `tracing-subscriber` or Prometheus crate. `store` and `router` are timed in
  `versions.rs` directly. `DEVPAGE_METRICS_PORT` (default 9030) in the
  config; `main.rs` installs the subscriber and serves the metrics router on
  its own listener.
- **`bench/`**: `devpage-bench`, a workspace member, `publish = false` —
  `corpus.rs` (xorshift-seeded documents as zips), `main.rs` (publish, open
  views on a Zipf curve), `report.rs` (scrape, stage deltas, quantiles,
  Markdown).
- **`deploy/server/Dockerfile`**: copies `bench/`. A new workspace member
  breaks the image build until it does — the Dockerfile copies members one by
  one.

## Where the difficulty was

Nowhere expected. The metrics took one file and no new crates; the bench
compiled clippy-clean on the second build. The only break was the
Dockerfile above. The real finding is the number: nothing beside the link
check matters yet.

## What the design got wrong or left open

- **`render` misses half the views.** A doc's page goes through
  `Resolver::show`, not `render::page`, so the span saw 1,654 of 3,000 views.
  The stage needs a span at the one place every kind is rendered.
- **`render` and `nav` nest.** `nav::tree` runs inside `render::page`, so
  render's time includes nav's. The design's table reads as if stages were
  disjoint; the report should say they nest, or the spans must not.
- **`check` includes rendering.** The check renders the page and every page a
  `#heading` link reaches, so `check` holds most of the Markdown rendering.
  Correct for "where does the time go", but whoever optimizes `render` will
  find it under `check`.
- **The image build copies workspace members by name** — every new crate is a
  Dockerfile edit, which the design's reach didn't name.

## What is left to be production-quality

- The other stages: `fetch`, `unpack`, `publish-check`, `upload`, `download`;
  requests by route and status; the totals (versions held, threads, open
  pages, cache bytes/hits/misses/evictions, resident memory).
- The envelope's corpus: 10,000 documents, 50,000 versions, slides and
  designs, devpage's own docs among them, sizes with the long tail.
- The other scenarios: misses, writes, herd, open pages, soak, knee; three
  runs and the median.
- `bench.yml`: compose with RustFS, the pod's limits, the report on the
  summary page, the artifact, main's last run downloaded and compared.
- `kubernetes.yaml`'s requests and limits, the metrics port kept out of the
  Service.
- Tests: the metrics port's own route test (nothing on `DEVPAGE_PORT`
  answers `/metrics` is checked by hand only), a config test for
  `DEVPAGE_METRICS_PORT`, the bench's open-loop scheduling.
- The docs' test counts and the configuration reference, once built.
