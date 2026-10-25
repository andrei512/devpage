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
into internals. Every subject offers a path down through five layers, each
answering a different question:

| Layer | Question it answers | What it holds |
| --- | --- | --- |
| **1. Landing** | What is this, and where do I go? | The subject's README: what it is and why it exists in a few paragraphs, one picture of its parts, and Topics grouped by what the reader wants. No mechanics. |
| **2. Essentials** | How should I think about it? | One or two articles that teach the mental model: the few core concepts, how they fit, and one concrete example walked end to end. A reader who stops here can reason about the subject correctly. |
| **3. Concepts and tasks** | How do I work with it? | Articles organized around what a reader wants to understand or do — "Working with state", "Navigating between screens", "Adding a component" — crossing mechanisms and linking down into them. Task articles give concrete, checked steps. |
| **4. Mechanisms** | How does this part work inside, and why? | One page per mechanism: its design, the reasons for it, its failure modes. Written for someone about to change it. |
| **5. Reference** | What exactly is the contract? | Schemas, APIs, configuration, vocabularies — complete and exact, reachable from any layer. |

Rules that make the hierarchy work:

- **Don't skip a layer.** A mechanism page with no essentials and concept
  articles above it is the "high level, then straight into design" failure:
  readers can find the detail but cannot tell why it matters or how it fits.
  When a layer is missing, write it before adding more depth.
- **Layers follow the reader's question, not folder depth.** A small subject
  may hold landing, essentials and mechanism on one page; a large one splits
  each layer into pages. The system as a whole has its own landing and
  essentials above the subjects'.
- **Every page opens with its title and an abstract** — one plain paragraph
  saying what the page explains. Abstracts are what make a list of links
  readable.
- **Topics connect the layers.** A page lists its children in a `## Topics`
  section at its end, grouped under `###` headings by layer or reader goal
  ("Essentials", "Working with state", "Mechanisms", "Reference"), each link
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
the layer each page belongs to, reader question, parent entrance, next
destinations and acceptance criteria. Resolve missing scope before changing
shared entrances or somebody else's subject. Do useful read-only investigation
while a question is pending.

## Ground, write, follow

Inspect implementation and relevant tests before stating behavior. Use the
project's nouns, as the owning box's glossary defines them, and keep one name
per concept everywhere. Distinguish what runs from what is a design, an
extension point nothing uses yet, or a test fake.

Write the page around its reader question and its layer. Establish what the
subject does, show a concrete situation when it helps, and link to the owners
of deeper mechanisms. Shorter copy is useful only if it preserves the behavior
the reader needs. State a reason only when the code, a constraint or existing
docs give it, phrased as the failure it prevents; never invent rationale. Do
not turn provisional claims into facts to make a story complete.

Exact contracts stay beside their definitions in code; a page explains and
links to them, and does not keep a hand-copied inventory that drifts from the
code unless that page is the reference layer and is checked against the code.

Follow the links from the landing page down through the changed pages. Read
edited files back before reporting them as written.

## Hand back a bounded result

Report the reading path, changes, source evidence for significant claims,
checks actually run, and unresolved contradictions or coverage. Leave review
judgment to a different agent using the [review skill](../review/SKILL.md).
Do not mark a subject complete when a layer, examples, incoming links or an
explicit acceptance criterion are missing.

Prepare assignments before dispatch; launching agents requires the operator's
authorization.
