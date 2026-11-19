---
id: "03"
blocked-by: ["01", "02"]
boxes: [engineering/tests]
---
# Establish which changes can affect each suite

Represent the existing browser groups, the sidebar integration coverage, adapter checks and the deploy-server PDF smoke as selectable targets. Declare feature and shared inputs in Nx. Keep a conservative broad group where ownership overlaps. Use one source of truth for input ownership and use it to distinguish known zero-browser changes from unclassified production changes.

## Acceptance

- Repeatable selection checks cover recorder-only → video; slide-PNG renderer → PNG and video; sidebar-only → sidebar and relevant integrations; standalone review brief → zero. Export suites stay idle for the scoped sidebar edit.
- Classify shared rendering, global styles, embedded assets, generated design bindings, browser launch code, test helpers, toolchain and lockfile changes according to the consumers they affect. A shared harness selects all dependent browser checks.
- A change to a real docs deck, font or other fixture selects every suite that reads it. The deployed PDF smoke includes its Docker inputs as well as application and rendering dependencies.
- Mixed changes take the union. Additions, deletions and both sides of renames are classified. New production files with unknown ownership select broad coverage, including the smoke when its scope is uncertain. A new browser test cannot disappear because it lacks an assigned filter.
- Reconcile the union of selected Rust tests with the full test inventory by enumeration. Include cases that demonstrate zero selection and conservative fallback independently.
