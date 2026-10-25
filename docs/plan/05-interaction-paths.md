---
id: "05"
blocked-by: []
boxes: [viewer/designs, viewer/design-engine]
---

# An interaction's path is relative to the file that wrote it

An `onClick` path — `navigate`, `overlay` — is resolved against the `.design`
file the attribute is written in, as every other path in a design is, so an
interaction inside an imported design works wherever that design is used.
The resolver stores the target as an address in the served folder, and the
canvas uses it as given. A target that doesn't exist, or lies outside the
served folder, is a problem at the attribute's position.

devpage's own docs embed a design, so `docs/devpage.yml` watches `.design`
files and the images designs use, and an edit to the embedded design updates
the page's problems.

## Acceptance

- A resolver test: a design imported from another folder carrying its own
  `navigate` resolves to the address beside the imported file; a missing
  target is a problem with its position.
- A Chrome test (`--ignored`): clicking an interaction inside an instance from
  another folder opens the right design.
