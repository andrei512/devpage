---
name: design
description: Work out how the system should change, until the understanding is shared — then write it into the design docs.
disable-model-invocation: true
---

# Design

Design is the conversation that works out how the system should change. It comes
before any code, and it produces one artifact: an update to the design docs.

Those docs live in `docs/boxes/`, one directory per **box** — a component of the
system. Its `README.md` describes how the box works and lists that box's
**constraints**; each constraint has its own file under `constraints/`. Review
checks a diff against them. The format is described in `docs/boxes/engineering/sdlc/`.

## Talk until told to act

A design session is a conversation, and it stays one until the operator asks for
the write. Enthusiasm, a change of topic, or an obvious-looking next step are not
permission.

Everything agreed gets written once, at the end. Editing docs as individual
points land fragments the design and pulls attention out of the conversation.

## Alignment

Read and use the [align skill](../align/SKILL.md) throughout the design
conversation. Settle what should change, why, and what its consequences are;
bring blast-radius findings back through the same conversation.

## The recommendation is the shape that passes no-knowable-rework

When options differ in size, recommend the one where no rework is already
nameable ([no-knowable-rework](../../../docs/constraints/no-knowable-rework.md)) —
judged against the design and the vision, not the current ticket. A smaller
shape appears only as a named cost, never as the proposal.

## Challenge real problems only

Disagree when something is genuinely wrong, and say why. Before raising an
objection, check the problem can actually happen in this system — an objection
invented to sound rigorous costs a round trip and teaches the operator to
discount you.

## Ground recommendations

When recommending an approach, say whether it's standard practice or your own
synthesis. If it deliberately departs from what most people do, say what the
departure buys and what it costs.

## Cut the branch

A design update describes a system that doesn't exist yet, so it can't go on
main — see [docs-are-true-on-main](../../../docs/boxes/engineering/sdlc/constraints/docs-are-true-on-main.md).
It lives on its own branch, one per design update, until the code catches up.

Cut that branch once the shape of the change is clear, before the first blast
radius pass.

Name it `design/<name>`. CI keys on that prefix — the
[SDLC design](../../../docs/boxes/engineering/sdlc/branches/README.md) has it run
on pushes to `main`, `design/**` and `prototype/**` — and tickets merge into the
design branch without a pull request, so a design branch named anything else
gets no CI on push, and every ticket merged into it goes ungated until the pull
request to main.

The branch is deleted, locally and on origin, once it merges into main, with
every ticket branch under it — [the SDLC design](../../../docs/boxes/engineering/sdlc/README.md)
keeps the history in the merge commits, not in the names.

When the design being settled is an amendment to one already in flight — a gap
that building turned up — it stays on that branch rather than getting one of its
own; it belongs to the same change set.

## Blast radius

A decision becomes a design change only once you know what it disturbs. Work out
and report:

- **Reach** — which boxes, docs and code it touches, and what has to move with it.
- **Contradictions** — which existing constraints or docs now disagree with it.
- **Risk** — what breaks, and which invariants are exposed.
- **Leftover** — what work it leaves behind for later.

Then take the findings back to the conversation.

## The loop

A session cycles: idea → debate → blast radius → debate. Each pass either turns
up something that wasn't accounted for, in which case it continues, or it turns
up nothing new. The operator decides when the design is settled.

## The write

Update each affected box's README: how the box works, and its constraints list.
A new constraint also gets its own file under that box's `constraints/` — the
rule stated precisely, why it exists, and how to check it — linked from its
one-line entry. A box that doesn't exist yet is created here.

A rule that holds everywhere is not that box's, however it came up. It goes in
`docs/constraints/`, which review reads on every diff. Ask which box could
violate it: if the answer is any of them, it belongs there. Filing a general
rule under whichever box happened to surface it is how one layout ends up
escaping raw HTML differently from the others while the constraint saying how
HTML is escaped sits in a file about the renderer — and review of the layout
never reads it.

Read the files back to confirm the edits are there before telling the operator
they're written.

The reason the design changed goes in the commit message, not into a new
document.

Docs carry their own reasons: state the failure mode a rule prevents, never
the predecessor system where it was learned. History lives in conversation
and commit messages; a design doc that cites an ancestor is leaning on
context its reader doesn't have.

## Done when

- The operator agrees the understanding is shared.
- A blast radius pass turned up nothing new.
- The box docs on the branch describe the system as it will be.
- The work left behind is named but not yet measured — `/prototype` spikes it,
  and `/plan` works from what that cost rather than from a guess.
