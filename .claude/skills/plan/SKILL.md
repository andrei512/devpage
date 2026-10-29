---
name: plan
description: Turn a settled design into an ordered set of tickets that agents can build in parallel.
---

# Plan

Planning turns a **prototype** into an ordered set of tickets. The design said
what should be true and the prototype found out what making it true costs, so
this is mostly deciding what it takes to productionise something that already
runs — and plans come out proportional to work that was measured rather than
imagined.

Start from the prototype's report at `docs/plan/00-prototype.md`: what it
touched, where the difficulty was, what the design got wrong, and what is left
to be production-quality. That last list is the plan's first draft; editing it
beats deriving one.

## Plans are about contracts

A **contract** is an interface between parts of the system or with its users:
the extended Markdown syntax authors write, a layout and its options, a CLI
command or flag, the config file format, the extension API, the HTML structure
and CSS hooks themes rely on.

Planning works out which contracts change, in what order, and what keeps the
system working in between. Everything else — which files, which functions — is the
worker's business, not the plan's.

## The system works between every ticket

A contract that already has callers can't simply change. Land it in three moves:

1. **Expand** — the new contract appears beside the old one; nothing uses it yet.
2. **Migrate** — callers move across in batches, each batch a ticket.
3. **Contract** — the old path is deleted.

The system works at every step; the price is that one side is temporarily dead
code. The deletion is its own ticket, never a "clean it up later" — the design
branch merges to main claiming one system, and a leftover old path makes that a
lie.

Documents and configs in other people's repositories are callers a merge
doesn't migrate: a syntax, flag or config field can be removed only after a
release that warns about it. Say in the plan which moves need that release.

## Cutting tickets

Cut where the contracts are: a ticket's inputs and outputs are contracts the
design already pinned, and everything inside it is one worker's business. That's
what lets several agents work at once — they depend on the contracts, not on each
other's code.

- Size it like a pull request you'd actually want to review: small enough to hold
  in your head, big enough that reviewing it is worth the trouble.
- **Err large.** Every ticket carries a fixed cost — a worker reading the design,
  a review, a merge — and below some size that ceremony outweighs the work. Ten
  lines of config is not a ticket. If two tickets touch one module, or one is
  meaningless without the next, they are one ticket.
- Every ticket leaves the system working on its own, using stubs or fakes behind a
  pinned contract where its neighbours don't exist yet.
- Give a ticket a blocking edge only where a contract genuinely couldn't be pinned
  in advance. Everything else runs in parallel.
- Work that makes the change easier — a move, a rename — becomes its own ticket,
  first.

## Test the order by narrating it

Walk the tickets top to bottom and say, in one sentence each, what the system does
once that ticket has landed. If a sentence comes out as "it's broken until the next
one", the cut is in the wrong place. This catches a bad plan before any code.

## What the operator approves

The operator approves the **story**, not the tickets: one line per step, naming the
contract that moves and what the system does afterwards.

    1. Parser accepts `:::note`; `> [!NOTE]` still works.   → unchanged, new syntax dormant
    2. Renderer draws both through one callout block.      → identical pages
    3. `> [!NOTE]` warns, pointing to `:::note`.           → authors migrate
    4. `> [!NOTE]` removed.                                → one syntax, docs true

Draft the tickets before presenting the story, so when they ask what's in step two
the answer already exists.

Read and use the [align skill](../align/SKILL.md) when discussing the story
with the operator: settle the order, the behavior after each step, and any
tradeoffs. Their approval of the story remains the gate to build.

## The write

Tickets live on the design branch and disappear when it merges — working
documents, not history. One file per ticket under `docs/plan/`, numbered in
dependency order.

Each opens with front-matter, so a workflow can read the plan as data rather than
parsing English:

```yaml
---
id: "02"
blocked-by: ["01"]
boxes: [renderer, layouts/slides]
---
```

Quote the ids — bare `02` parses as the number 2, and then nothing joins a
ticket to the `02-` file it lives in. They match the filename prefix exactly. A
ticket with nothing blocking it carries `blocked-by: []` rather than omitting the
key, so a reader never has to tell "none" from "unstated". Boxes are named by
directory under `docs/boxes/`, nested ones by path (`layouts/slides`).

State those two facts here and nowhere else — the prose says what to build, the
front-matter says how it is wired, and neither repeats the other.

`/build` reads `blocked-by` to work out the frontier. Nothing reads `boxes` yet;
it is there because review derives boxes from a diff's paths today, which misses
a box a change *should* have touched and didn't.

Each ticket says what to build in terms of behaviour and lists its acceptance
criteria. Its boxes and blockers live in the front-matter and are not repeated in
the prose. No file paths or code snippets: they go stale before the ticket is
picked up.

## Done when

- The operator approved the story.
- Narrating the order gives a working system at every step.
- Every ticket has acceptance criteria and its blocking edges.
- The deletions finishing each expand–migrate–contract have their own tickets.
