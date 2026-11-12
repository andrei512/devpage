---
name: documentation
description: Author or reorganize this project's system documentation from its implementation, following a reading hierarchy that goes from landing pages through essentials and concept/task articles down to mechanisms and reference. Use for system explanations, guides, documentation restructuring and their reading paths; use review for independent evaluation.
---

# Documentation

Make a subject understandable from its landing page, then let readers go down,
one layer at a time, to exactly the detail they need. Work in the operator's
selected checkout. An assigned writing task authorizes its scope; discussion
alone does not.

## The reading hierarchy

Good platform documentation never drops a reader from "what is this" straight
into internals: a reader goes down one level at a time.

The page types are in `docs/structure/`, one file each. A type file says what
a page of that type is for and which types it may sit under; nothing here
repeats it. Read the type file before writing a page of that type. Every page
names its type in front matter (`page-type: mechanism`) and has exactly one,
and `devpage check docs` reports a page with no type, a page no Topics section
lists, and a page under a parent its type does not allow. When a subject lacks
the page a type must sit under, write that page before adding more depth.

Rules the type files do not hold:

- **Every page opens with its title and an abstract** — one plain paragraph
  saying what the page explains. Abstracts are what make a list of links
  readable.
- **Topics connect the pages.** A page lists its children in a `## Topics`
  section at its end, grouped under `###` headings by type or reader goal
  ("Essentials", "Working with state", "Reference"), each link
  followed by a one-line abstract. The order of the list is the reading order.
- **Link both ways.** Each page links up to where it sits and down to the next
  useful detail. Essentials and concept articles link to the mechanisms that
  implement them; mechanisms link to the reference for exact contracts.
- **Pictures go high.** Landing and essentials pages earn diagrams most: the
  parts of a subject, the path of one request, the life of one piece of data.
  Mechanism pages use diagrams for internal flows.
- **Diagrams are navigation.** A diagram node that stands for something with
  its own page or section links to it with a Mermaid `click` line —
  `click engine href "./engine/README.md" "Engine loop"` — so a reader goes down
  a layer by clicking the part they want to understand. Link to the page that
  explains the node (or to a `#heading` on the same page); leave a node
  unlinked when nothing explains it further. Write each `click` line with
  single spaces, and check every target resolves. Only flowcharts (and
  `graph`) can carry links — a `click` in a sequence diagram is a parse error —
  so draw a diagram whose job is navigation as a flowchart, and keep sequence
  diagrams for the order of an interaction.

## Establish the assignment

Read the assignment and the project's SDLC design under
`docs/boxes/engineering/sdlc/`, which says where documentation lives and how
boxes are organized; do not create a second set of those rules.

For a specific edit, use the requested subject and its incoming reading path.
For a delegated subject, require an explicit write set, source/code to inspect,
the type of each page, reader question, parent entrance, next
destinations and acceptance criteria. Resolve missing scope before changing
shared entrances or somebody else's subject. Do useful read-only investigation
while a question is pending.

## Ground, write, follow

Inspect implementation and relevant tests before stating behavior. Use the
project's nouns, as the owning box's glossary defines them, and keep one name
per concept everywhere. Distinguish what runs from what is a design, an
extension point nothing uses yet, or a test fake.

Write the page around its reader question and its type. Establish what the
subject does, show a concrete situation when it helps, and link to the owners
of deeper mechanisms. Shorter copy is useful only if it preserves the behavior
the reader needs. State a reason only when the code, a constraint or existing
docs give it, phrased as the failure it prevents; never invent rationale. Do
not turn provisional claims into facts to make a story complete.

Exact contracts stay beside their definitions in code; a page explains and
links to them, and does not keep a hand-copied inventory that drifts from the
code unless that page is a reference page and is checked against the code.

Follow the links from the landing page down through the changed pages. Read
edited files back before reporting them as written.

## Hand back a bounded result

Report the reading path, changes, source evidence for significant claims,
checks actually run, and unresolved contradictions or coverage. Leave review
judgment to a different agent using the [review skill](../review/SKILL.md).
Do not mark a subject complete when a page its type must sit under, examples, incoming links or an
explicit acceptance criterion are missing.

Prepare assignments before dispatch; launching agents requires the operator's
authorization.
