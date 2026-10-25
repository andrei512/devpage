---
name: review
description: Review a change against the bar its target sets — a ticket going into a design branch, or a design branch going into main. Use when reviewing someone's branch or pull request, checking your own work before handing it back, or deciding whether a branch is ready to merge.
---

# Review

Review checks work against what it was supposed to do. Which bar applies is
decided by where the change is going — and a ticket arrives as a branch rather
than a pull request, since the review is this conversation and not a comment
thread.

## The general constraints, always

`docs/constraints/` holds the rules that belong to no single box. Read that
index on every review, whatever the diff touches — they are the ones nobody
thinks to look for, because there is no box whose name matches them.

## Architecture review

When reviewing architecture, use the
[improve-codebase-architecture skill](../improve-codebase-architecture/SKILL.md)
after first reading its [devpage integration note](../improve-codebase-architecture/UPSTREAM.md).
Keep the review within the bar and scope below; implementation and design writes
still require the operator's authorization.

## Which boxes the diff touches

Every path in the diff belongs to a box, and boxes nest — code inside a box that
sits within another is bound by both of their constraints.

Start by listing the boxes the diff touches and reading each one's README,
including its constraints list. Each entry there states a rule in one line and
links to the file explaining it. Open that file when the diff plausibly bears on
the rule; the one-liner is enough to decide whether it does.

Code whose area hasn't been designed yet has no box. It is bound by AGENTS.md
alone — that's a fact about the diff, not a finding.

Those constraints are the record of decisions already taken about that part of
the system. Review is what stops them being broken quietly; nothing else does.

## Verify, don't read

Check the head out in a worktree and run things — the gates, the acceptance
criteria, the case you think is broken. A review that only reads the diff is
guessing.

When a change has nothing to run — docs, a plan, a skill — verifying means
following it instead: do its references resolve, does it contradict another
document, could someone act on it without having been in the conversation.

## Show it, or it's a question

A finding names what is wrong and shows it: a command that fails, with its input
and the wrong result — or, where nothing executes, the file and line that says
otherwise. Anything you can only argue for is a question; ask it as one.
Speculation dressed as a finding teaches the operator to ignore reviews.
When a question needs discussion with the operator, read and use the
[align skill](../align/SKILL.md) to clarify the intended behavior or requirement.
The review bar and the requirement to demonstrate findings stay the same.

## Reachable by accident, or it isn't a finding

A finding is something a competent person hits by mistake. If triggering it takes
deliberate evasion — aliasing an import to dodge a name check, reaching outside a
scanned directory, doing by hand what the tooling exists to do — it goes in the
report as a note and changes nothing.

The authors here are the operator and agents working from written briefs. Guarding
against a saboteur costs real time and buys nothing, and each such round makes the
next review cheaper to ignore.

## Stay inside the bar

Review what the ticket promised and what the touched boxes require. That is the
whole mandate. An agent told to keep attacking until something breaks will always
find something, and the tenth finding is never worth what the first one was.

## Judge the artifact

Read the diff, not the author's account of it. What you were told about a change
is what its author believes they did; the code is what they did.

## A constraint in the way is a design problem

If the code can only work by breaking a constraint, that isn't a comment telling
the author to try harder. It means the design and the code now disagree, and
resolving that is design's job. Say which constraint, why the code needs it
broken, and escalate. The design session uses `align` with the operator to
settle the conflict; the reviewer does not amend the design.

## The task bar — a ticket going into the design branch

- Every acceptance criterion, run and passing.
- Every constraint the diff bears on, checked the way its file says to check it.
- The architecture rules in AGENTS.md for the code it touches.
- The gates green, run by you rather than taken on trust.
- The system still works: nothing that worked before the ticket is broken by it.

## The drift bar — the design branch going into main

The question here is whether the code now does what the design docs claim.

- Every constraint the design added actually holds in the code.
- Each expand–migrate–contract finished: no old path left beside its replacement.
- What the docs describe is what a reader of the code would find.

This is the review that keeps main's docs true. Nothing else checks it.

## Reviewing your own work before handing it back

You can't judge your own work — you know why every line is there, so you read
intent instead of code. What you can do is mechanical: run the gates, tick each
acceptance criterion, walk the constraints you know you touched. Leave the
judgement to the fresh reviewer.

## A grown surface is enumerated, not skimmed

When the diff grows a surface its users consume — a Markdown extension, a
layout or layout option, a CLI command or flag, a config field, an extension
API hook, a CSS hook — enumerate the new items and check each one is documented
where its users are taught (the owning box's README or its reference). Silence
is the finding: "docs true" is satisfiable by saying nothing, and an
undocumented capability is one nobody uses and a later change reinvents.
Where a mechanical coverage check exists, verify it actually enumerates the
grown surface rather than checking a hand-kept list.

## Documentation review

For documentation changes, start from the parent page and follow the reading
path the change promises before reading implementation detail. Check the
significant claims against code and real wiring, and check what the subject
omitted, not only what it says. Do its links resolve; does it contradict another
document; could someone act on it without having been in the conversation.

Distinguish incorrect behavior, broken navigation, missing coverage and editorial
problems in the report. Demonstrate each with the source, failed check or exact
reading transition. Keep author and reviewer separate.

## A softened design claim is a finding

A diff that weakens, defers, or limits what a design doc promised — without
that change coming from the operator's own design conversation — is a
finding, however green the tests are. The bar is "does the code do what the
docs claim", and a lane that edits the claim to fit the code has answered
the question backwards.

## Done when

- Every finding is demonstrated, or dropped.
- The bar's checks have each been run rather than assumed.
- Each comment is specific enough to act on without asking what you meant.
