---
id: "01"
blocked-by: []
boxes: ["viewer/live-session/agents/world/extending"]
---

# Make registration and writes atomic

- A module that fails registration exposes none of its functions, kinds or widgets.
- Overlapping changes to a part preserve both updates. A failed function commits neither state nor entries.
- Bring core and extension history into a common event stream. Existing event numbers, agent conversations and migrated session data survive restart. Verify with fixtures from both storage generations.
