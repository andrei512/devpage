---
page-type: process
---
# Checking the envelope

How to find out whether a change still lets
[one server hold the envelope](one-server-holds-the-envelope.md): run the
server's benchmark on the change and read each target as met or missed.

## When

Before a change to how a page is served, published, cached or kept in memory
merges. An agent runs it alone; a person is needed only when a target is
missed.

## Steps

1. Push the branch, then start the benchmark on it:
   `gh workflow run bench.yml --ref <branch>`. The defaults — seed `1`, scale
   `1`, three runs of each scenario — take about five minutes
   ([the server's benchmark](../../../../engineering/tests/README.md#the-servers-benchmark)).
2. Wait for the run to end: `gh run watch`.
3. Open the run's summary page. The report lists each target of the
   [envelope](../measuring-a-server/README.md#the-envelope) as met or missed,
   beside `main`'s last run.
4. Compare each number with `main`'s: a target that is still met but has
   moved a long way toward its limit is worth a line in the report.

## Report

Put the run's link in the pull request. With every target met, that is all.
A missed target is a regression, whatever else the change improves: say which
target, by how much, and against which number on `main`, and stop — the
change does not merge until a person decides.
