---
id: "05"
blocked-by: ["04"]
boxes: [engineering/tests, engineering/developing-devpage, engineering/sdlc]
---
# Switch local development and CI to the verified selection

Replace the blanket local browser gate and unconditional Windows browser command. Keep the Windows browser step on the existing runner so it reuses the test binary. Use the same change classification for the independent Docker PDF smoke, preserving its own execution environment. Retire the temporary comparison path. Keep a manual full run and a scheduled full check to detect selection drift.

## Acceptance

- A brief-only PR launches zero browsers across both workflows. A recorder edit runs video. A PNG renderer edit includes the video consumer. A sidebar edit selects the relevant page coverage. Shared and unclassified changes receive the broad coverage established in the preceding tickets.
- Required GitHub checks reach a successful, visible conclusion for a verified empty selection. Check branch protection and any workflow-level path filtering before switching. If comparison history is unavailable, run broad; if the selector itself fails, surface the failure.
- Contributor instructions, test documentation and agent review guidance use the same affected command. Document explain, full run and how to add a new feature, test or fixture to the selection inputs.
- Verify ordinary Cargo tests, formatting, lint, docs checks, Rust-only install and release paths keep their existing behavior. Compare measured post-cutover runs with the baseline. If a missed dependency appears, the full-run override can restore coverage while its input declaration is corrected.
