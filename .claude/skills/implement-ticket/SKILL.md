---
name: implement-ticket
description: Build one ticket end to end — red-green, gates, its own review, a branch ready to merge. Use when picking up a ticket from a plan.
---

# Implement a ticket

Build exactly one ticket, on your own branch off the design branch, get it
reviewed, and hand back a branch ready to merge.

Work in your own tree — a worktree or clone the coordinator named. If you
find yourself in the operator's checkout, do not run `git checkout` in it:
their runtime builds from whatever is checked out there, and moving it
changes what their next build produces. Stop and tell the coordinator
instead.

The prototype's code is already there — you are hardening something that runs,
not starting from nothing. Read it before you read your ticket; it is the most
honest description of the system you have.

There is no pull request. The review happens between agents rather than in
comments, and the coordinator merges the branch; CI runs on the design branch
after it lands.

## Read first

The ticket says what to build and how it will be judged. The boxes it names have
READMEs and constraints — read them before writing anything, because the review
will check your code against them.

## Red before green

Write the test that fails the way the ticket describes. Run it. Watch it fail for
the reason you expect — a test that goes red for the wrong reason is worse than no
test. Then write only enough code to make it pass.

## Keep the loop tight

While working, run only what tells you something: one module's build, one test
file. Save the full gates in AGENTS.md — every target's build and the whole
suite — for the end. A ninety-second loop gets skipped; a two-second one
doesn't.

## Stay inside the ticket

Build what the ticket asks and nothing else. Anything you notice outside it — a
bug, a smell, a missing test elsewhere — goes in your report as a note, not into
the diff. Unasked-for changes are what make a review hard to trust.

## Leave the system working

The branch has to work on its own. Where a contract exists but the thing behind it
doesn't yet, use a stub or a fake rather than half-wiring the real one.

## When the design didn't anticipate something

If there's one sensible answer, take it and say so in your report. If there
are genuine alternatives with different consequences, stop and tell the
coordinator — deciding isn't your job. Include the gap, the alternatives, and
their consequences so the coordinator can use the [align skill](../align/SKILL.md)
with the operator. Keep that handoff; do not open a separate operator conversation.

## Get it reviewed before you hand it back

Run the mechanical half of `review` on yourself first: gates green, every
acceptance criterion ticked, the constraints you touched walked. You can't judge
your own work — you know why every line is there — so that half is all you can
honestly do.

Then review the rest of it yourself, against the `review` skill at the task bar:
the acceptance criteria run rather than assumed, the constraints of every box
your diff touches, `docs/constraints/` whatever you touched, and the gates green
from your own run.

You are the wrong person for this and it is still your job. You know why every
line is there, so you will read intent where a stranger reads code — which is
exactly how a guard that doesn't guard gets written. Compensate by attacking
rather than confirming: try to make each thing you built fail, and treat "I
couldn't break it" as the evidence, not "it looks right".

Fresh eyes come once, at the end, when the design branch goes to main.

## Code changes keep the docs true

A code change ends with rereading the docs that describe what it touched;
any sentence the change made untrue gets fixed in the same diff. A change
to a surface its users consume — Markdown syntax, a layout, a CLI flag, a
config field, the extension API — ends with the box doc that
teaches it updated in the same diff. Design writes are the deliberate
opposite — there the docs run ahead of the code until the build catches up.

## Rebase is integration, not ceremony

What landed on the design branch since you branched is other lanes' intent.
When you rebase onto the moved tip, read what arrived in the files you share,
preserve it, and re-run **your ticket's own proof** on the result — a green
suite only protects what a test pins, and the line most likely to die in your
resolution is a neighbor's one-line wiring.

## Done when

- Every acceptance criterion passes.
- The gates are green.
- You attacked your own work and say in your report what you tried.
- Your report says what changed, and what you noticed but left alone.

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
