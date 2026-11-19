# Affected browser tests for devpage

**Decision requested.** Adopt Nx for choosing browser checks from a Git diff. Cargo builds and runs the selected Rust tests in one process. The same selection drives local development and CI.

## What we established

An isolated clone using Nx 24.0.6 and two targets selected video for a recorder edit, both video and slide PNG for a PNG renderer edit, both for shared CSS, and neither for a root README edit. A hypothetical unlisted renderer also selected nothing. Trustworthy classification is the main implementation cost. The prototype demonstrated selection only. A separate enumeration of the compiled Rust test binary showed that two filters together select the expected 12 video and PNG tests.

The research baseline counted 117 ignored tests in 11 browser modules and three additional ignored Chrome adapter checks. Actual inventory varies by platform and revision. Some suites consume real docs as fixtures, the world suite has Unix-specific coverage, Windows skips GPU tests, and narrated video can depend on credentials. The Docker deploy-server smoke also prints a PDF. These are inputs to the plan.

In six sampled Windows runs, Chrome took 55–74 seconds. A packaging-only PR paid 62 seconds. The existing Mac benchmark puts the warm full browser run around 11 seconds on the measured machine. Compare like-for-like before claiming a saving, especially when bootstrapping Node on a runner. [Sample packaging PR](https://github.com/andrei512/devpage/pull/81)

## Rollout

1. Preserve the test inventory while giving sidebar behavior its own group.
2. Add an opt-in Nx selector and a single-process Cargo runner. Developers can inspect a decision before running tests.
3. Connect production code, test fixtures and shared behavior to the affected suites. Verify targeted, broad and empty selections.
4. Compare selections on real diffs while the existing CI gate still runs.
5. Switch the Windows gate, Docker smoke selection and developer instructions together. Retain an explicit full run.

Cargo build caching, release automation and product installation stay outside this migration. Scope includes the deploy-server PDF check because it launches Chrome independently of the Windows gate. Approve the rollout before implementation.
