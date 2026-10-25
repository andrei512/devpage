---
name: prototype
description: Spike a settled design to find out what building it actually costs. Use when the design session hands off to a spike.
---

# Prototype

A design says what should be true. A prototype finds out what making it true
costs — which layers move, where the difficulty really is, and how much work
there honestly is. Planning without that produces nine tickets for an
afternoon's work.

The operator asks for the spike. A settled design is not permission to start
one — this writes code for half an hour and merges to the design branch, and
permission to write is stated rather than inferred.

Once asked, the design session **forks itself** to run this and relays what
comes back — a fork carries the conversation that produced the design, which is
the thing a spike needs most, and its whole life is the spike, which is what the
clock and the committed report both want.

## What you are building

**One end-to-end path** through whatever the design newly brings. Not the
plumbing around it, not every case — the new mechanism, working once, from one
end to the other.

Real code with tests. A prototype means less ceremony, not worse work: no
tickets, no branch per piece, no review round. The code is meant to survive.

Work on `prototype/<name>`, cut from the design branch. It is short-lived: you
merge it back into the design branch yourself when the spike is done, and the
lanes take over from there. CI runs on it and the gates in AGENTS.md apply — a
spike nobody can build teaches nothing.

## Thirty minutes, measured

You have thirty minutes of wall clock. **Read it off a clock** — `date` before
you start, and again after every build or test run, which is the natural cadence
and the place the time actually goes. Never estimate it; estimates of your own
speed are worthless in both directions.

Waiting on a compiler counts. It is wall clock, not effort.

**At thirty minutes you stop where you are**, mid-step if that's where you are,
and report. The budget is the measurement — running over destroys the only
number this stage produces.

Your instruction is to **keep going**. "This is bigger than I thought" is a
reason to continue, not to report. Take the obvious next step rather than asking
whether to.

Come back before the budget only for a design **contradiction** — something the
design asserts that cannot be true, where continuing would build on a lie. Size
is never that reason.

You don't amend the design yourself. What you found goes in your report, and the
operator settles it as a design amendment on the design branch — the same path
any mid-build discovery takes. The design session relaying the report reads
and uses the [align skill](../align/SKILL.md) to discuss the contradiction
or the scope that needs re-cutting; the prototype does not bypass that handoff.

## When the budget runs out

If there is no end-to-end path when the time is gone, **the design update is too
big**. That is the finding, and it is evidence rather than an opinion.

Report how far you got and what is still untouched. Do not propose the split —
re-cutting a design update is a design decision, and it goes back to that
conversation.

## What you hand back

**Commit the report on the branch, as `docs/plan/00-prototype.md`.** A message to
whoever spawned you doesn't reach the planning session; a file on the branch
does, and `/build` already deletes `docs/plan/` before the design branch reaches
main.

It is the plan's first draft. Four things:

- **What you touched** — which boxes, which modules, which layers moved
  (parsing, rendering, layouts, the live server).
- **Where the difficulty actually was**, as opposed to where anyone expected it.
- **What the design got wrong** — anything it asserted that turned out false,
  underspecified, or contradictory.
- **What is left to be production-quality**: the cases you skipped, the paths you
  stubbed, the tests you didn't write.

No diff walkthrough. Nobody reads a prototype's diff; they act on what it cost.

## Done when

- One end-to-end path through the new mechanism runs, with tests.
- The gates are green.
- The report is committed at `docs/plan/00-prototype.md`.
- The branch is merged into the design branch and deleted. You do that, because
  you are the only agent alive at that moment and the report is unreadable from
  anywhere else — `/plan` runs on the design branch, not on yours.

## Never invent spec

A lane writes docs to record what it built and to correct sentences its own
change made untrue. It does **not** change what the design *says*: weakening
a claim, deferring a promise ("v1 approximates…", "the full version arrives
later"), or adding a limitation the operator never agreed to is a design
decision wearing a doc edit's clothes — and writing it down makes the drift
review pass it, because the review asks whether code matches docs.

When the design cannot be built as written, that is an escalation, not an
edit: report it, and the operator rules. The doc changes in the design
conversation or not at all.
