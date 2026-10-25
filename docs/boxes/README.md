# The system catalog

Every part of devpage is explained here once, in the folder of the subject that
owns it. Start from the [vision](../vision.md) for where this is going; come
here for how the system works.

A box appears when its area is designed:

- [Engineering](engineering/README.md) — how changes reach main and how the
  system documents itself.

## How to read the catalog

Folders are the hierarchy. Each subject folder has a README that is its
entrance: it says what the subject is and why it exists, summarizes its parts
and links to each focused child. A focused child is a subdirectory with its own
README and explains one mechanism fully. Nesting describes conceptual
containment; a part that merely uses another does not contain it, so follow
links to reach the owner.

Beside the READMEs, a subject can hold:

- `constraints/` — one file per rule: the rule, why it holds and how it is
  checked. The subject's README lists them one line each.
- `guides/` — task-oriented pages with checked examples.
- `glossary/` — the words the subject owns, linked from where they are used.

## Constraints

Rules that belong to no single subject are the
[general constraints](../constraints/README.md), checked on every review. Every
other constraint lives with the subject it governs and applies to that
subject's descendants.
