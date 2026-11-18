---
id: "06"
blocked-by: ["05"]
boxes: ["viewer/live-session/agents/world/extending"]
---

# Remove superseded dispatch and state paths

- Remove duplicate built-in dispatch, hardcoded widget placement and combined snapshot handling after migrating their consumers. Retain promised configuration compatibility and legacy data import.
- Verify existing session history, voice, agent tasks, pages, canvas and extension workflows after restart. Run the Rust, Chrome and live-session checks and compare the running result against the design.
