---
name: documentation
description: Author or reorganize devpage system documentation from its implementation, and prepare bounded documentation assignments. Use for system explanations, guides, documentation migration and their reading paths; use review for independent evaluation.
---

# Documentation

Make a subject understandable from its parent page, then let readers follow its
behavior into the next useful detail. Work in the operator's selected checkout.
An assigned writing task authorizes its scope; discussion alone does not.

## Establish the assignment

Read the assignment and the [boxes and documentation](../../../docs/boxes/engineering/sdlc/README.md#boxes-and-documentation)
section of the SDLC design. It is authoritative for where documentation lives
and how it is organized; do not create a second set of those rules here.

For a specific edit, use the requested subject and its incoming reading path.
For a delegated subject, require an explicit write set, source/code to inspect,
reader question, parent entrance, next destinations and acceptance criteria.
Resolve missing scope before changing shared entrances or somebody else's subject.
Do useful read-only investigation while a question is pending.

## Ground, write, follow

Inspect implementation and relevant tests before stating behavior. Use the
project's nouns, as the owning box's glossary defines them, and keep one name
per concept everywhere. Distinguish what runs from what is a design, an
extension point nothing uses yet, or a test fake.

Write the page around its reader question. Establish what the subject does,
show a concrete situation when it helps, and link to the owners of deeper
mechanisms. A broad entrance summarizes; its child earns the additional detail.
Shorter copy is useful only if it preserves the behavior the reader needs.
Do not turn provisional claims into facts to make a story complete.

Exact contracts — the Markdown syntax an extension accepts, a layout's
options, a CLI flag, the config file's fields — stay beside their definitions in
code. A page explains and links to them; it does not keep a hand-copied
inventory of signatures that drifts from the code.

Follow the links from the parent through the changed pages. Read edited files
back before reporting them as written.

## Hand back a bounded result

Report the reading path, changes, source evidence for significant claims,
checks actually run, and unresolved contradictions or coverage. Leave review
judgment to a different agent using the [review skill](../review/SKILL.md).
Do not mark a subject complete when examples, incoming links or an explicit
acceptance criterion are missing.

Prepare assignments before dispatch; launching agents requires the operator's
authorization.
