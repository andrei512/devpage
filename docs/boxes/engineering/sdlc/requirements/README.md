---
page-type: reference
---
# Requirements

What the way changes are made must always do. Each requirement has its own page: the rule in
one sentence, why it exists, and how it is checked.

- [On main, the box docs describe the system as it is](docs-are-true-on-main.md)
  — anything merged to main leaves the docs an accurate description of the
  code; a design not yet built waits on its design branch.
- [Decisions live in box docs, not in a decision log](decisions-live-in-box-docs.md)
  — a decision goes into the owning box's docs or constraints and its reason
  into the commit message; there is no ADR directory.
