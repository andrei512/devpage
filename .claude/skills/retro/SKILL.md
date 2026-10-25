---
name: retro
description: After a design branch merges, report where the time went and what each stage caught, so the operator can see which stage is ceremony and which is too weak.
---

# Retro

Runs after the design branch merges into main, in the session that coordinated
the build — a fresh agent would spend its life reconstructing what this session
already holds. It is not part of the ready-to-merge report: the operator's final
review and merge come first, as the [SDLC design](../../../docs/boxes/engineering/sdlc/README.md)
lays out. It answers one question: **was each stage worth what it cost, this
run?**

A stage that caught nothing is ceremony; a stage that let something through for
a later stage to catch is too weak. Same question in both directions, and the
retro doesn't answer it — one run is not evidence. It surfaces; the operator
carries what repeats across runs and says when a rule changes.

## What it produces

A short report **in chat**. Nothing is written: no file, no ledger, no edits to
skills or constraints. If the operator decides something changes, that's a
design conversation on its own terms. When the operator wants to discuss a
finding or a possible process change, read and use the
[align skill](../align/SKILL.md) to explore what the evidence supports.
The report itself requires no alignment round; discussion authorizes no edits.

## The shape

One line per stage — its cost, and what it caught — then a closing paragraph
naming what looked off. The paragraph is the point; the table is what makes it
credible.

```
Design      4 rounds        killed a second parser + a layout option before any code was written
Prototype   8 min           corrected the design 3× (file watcher misses renames; SVG ids collide across diagrams)
Plan        1 ticket        —
Build       1 lane          self-review: 0 findings
Review      1 round         2 findings, 0 blocking

Plan wrote a document to say "do the one obvious thing." Self-review found
nothing here — second run in a row.
```

## The bar for a line

A number or a quote, never an adjective. "Design took several productive
rounds" says nothing; "killed the second parser twice before code" is a fact
the operator can weigh. Costs are measured — prototype minutes come from its
report, round counts from the conversation, ticket counts from the branch. A
stage with nothing to say gets a dash, which is itself the finding.

What belongs in the closing paragraph: work thrown away and why, a dispatch
that a written rule would have prevented, something the operator had to say
twice, a stage whose findings a cheaper stage should have caught. "Nothing
looked off" is a legitimate paragraph when it's true.
