---
id: "04"
blocked-by: ["03"]
boxes: [engineering/tests, engineering/developing-devpage]
---
# Validate selections on the real CI paths

Add advisory selection to pull requests and main, design and prototype pushes alongside the current gates. Use the PR target branch’s merge base, including PRs targeting design branches. For pushes use an available successful baseline on the same branch. Fetch sufficient history. Missing or invalid ancestry, including a new branch or force push, requests broad coverage.

## Acceptance

- A PR and a local checkout produce the same explanation for the same base and head. Exercise branch creation, a preceding failed run and a shallow checkout. Make the selected commits and any fallback visible.
- Replay representative historical diffs and compare them with independently reviewed expected suites. Exercise both Windows and Docker policies. Inspect every unexpected omission before enabling skips.
- Measure setup, selection and execution separately on an unrelated, focused and broad change. Compare Windows runner time and total CI critical path. Check whether batching avoids a regression on broad changes. Record the results and any corrected inputs.
- The comparison period preserves the existing required checks and produces a usable selection report even when zero browser suites are proposed.
