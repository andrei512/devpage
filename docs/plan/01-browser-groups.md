---
id: "01"
blocked-by: []
boxes: [engineering/tests]
---
# Make browser behavior individually selectable

Move sidebar-specific checks into a separately runnable Rust test group. Identify existing cold-load, widget mounting and server-mode integration checks that also exercise the sidebar. Where other suites import helpers from the page-script suite, expose shared test support so a change to a sidebar test has its own boundary. Preserve remaining suite boundaries.

## Acceptance

- Compare the old and new ignored-test lists on each supported CI test platform. Account for every renamed or moved test and every ignored Chrome adapter check outside the browser modules.
- The sidebar group can run alone. Document the additional integration filters needed for a sidebar change. Each filter selects at least one intended test on its applicable platform.
- Full and focused runs preserve existing GPU, OS and credential conditions. The same test matching two filters executes once.
