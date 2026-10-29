---
name: build
description: Take an approved plan and make it happen — dispatch tickets to workers, get each reviewed and merged, finish with the design branch ready for main.
---

# Build

Build takes the approved plan on a design branch and makes it real: it hands
tickets to workers, gets each one reviewed and merged, and finishes when the
branch holds the whole design.

The agent running this is a **coordinator**. It dispatches tickets, gets each one
reviewed by someone else, and merges — it never writes code, and it never reviews
the work it dispatched.

## The loop

0. Confirm the prototype has merged into the design branch. Lanes harden what it
   built; they don't start from an empty branch.
1. Work out the **frontier**: every lane whose blockers are already merged. The
   blockers are in each ticket's front-matter — read them from there rather than
   from its prose.
2. Hand each lane to a **fork**, in its own git worktree (or its own clone)
   on its own branch off the design branch, running the `implement-ticket`
   skill. **Never in the operator's checkout.** The operator's working copy
   is their runtime — the docs server they have open is built from
   whatever is checked out there — so a lane that runs `git checkout` in it
   silently changes what the operator's next build produces, and an operator
   who rebuilds mid-review loses the feature under review. A coordinator that
   cannot give a lane an isolated tree stops and says so rather than borrowing
   the operator's. A fork starts working rather than reading, which is most of
   what a briefed agent spends its time on — but it only carries the design
   conversation if you are the session that had it, so when you aren't, the
   brief has to carry what was decided.

   The prototype's code is already on the design branch by now, so a lane starts
   from something that runs and hardens it rather than starting from nothing.

   The brief names the other lanes in flight and any file two lanes both
   touch. A lane that doesn't know a neighbor exists resolves a rebase toward
   its own picture and silently drops the neighbor's intent — that is how a
   fully-tested feature ships with zero callers.
3. **The worker reviews its own work** at the task bar and comes back once,
   converged. Fresh eyes cost a spawn, a briefing and a round trip per ticket,
   and buy less than they do at the end — so they happen once, on the design
   branch going into main, where a stranger reads the whole thing rather than
   one diff.
4. Merge the branch into the design branch with `--no-ff`, then recompute the
   frontier and go again.

**A ticket doesn't get a pull request.** The review is agent-to-agent rather
than in comments, the merge is a merge, and branch protection only guards main —
so the only thing a pull request there would add is a CI trigger, and CI runs
on every push to a design branch instead. One pull request exists: the design
branch into main.

So a lane surfaces once, converged, rather than four times per ticket. The
coordinator's job shrinks to dispatching, merging, and carrying design questions
to the operator.

Spawn as many lanes as the machine can genuinely build at once; the frontier
being wide is an opportunity, not an obligation.

The loop converges because the bar is fixed, not because nothing more could be
said. A second round costs more than the last thing it found — decide that
deliberately rather than by habit.

## Merge the lane's branches yourself

A ticket going into the design branch is yours to merge once its review is
clean, with `--no-ff` and no pull request. Don't route it to the operator, or
tell them it happened — the only pull request they care about is the design
branch going into main.

What does reach them is a design question: a gap with more than one sensible
answer, which is theirs to settle and nobody else's.

## State lives in git

Which tickets are done is which branches are merged. Read the repository
rather than trusting your own account of it — a coordinator that dies halfway
through picks up again by looking.

## Never implement

Writing code inside the coordinator burns the context it needs to track
everything else, and turns the next review into a review of its own work.

## Merges are serial even though the work isn't

Two workers can finish at the same time; their merges still happen one at a time.
Rebase onto the design branch as it now stands, confirm the gates are green, then
merge. The design branch is green at every point.

## When a worker hits something the design didn't anticipate

If there's one sensible answer, take it and record it in the lane's report — a
gap with a single option isn't a decision.

Escalate only when there are genuine alternatives with different consequences.
Then stop that ticket and take it to the operator as a design conversation.
Read and use the [align skill](../align/SKILL.md) to settle the gap and the
consequences of each alternative. What they settle amends **this** branch —
it's part of the same change set, and the branch already describes a future
system. Don't decide it yourself, and don't wait for a second design branch to
reach main.

## Finishing

When every ticket is merged, delete `docs/plan/` — the tickets and the
prototype's report are working documents and must not ride the merge onto main.

Then open a pull request from the design branch to main and have a fresh agent —
one that neither wrote nor coordinated any of this branch — review it at the
drift bar: does the code now do what the design docs claim?

Its comments come back as new tickets, and the loop above resumes. When the
review is clean, hand the operator the design doc diff, the step story with each
step's outcome and the reviewer's verdict — printed in the chat message itself,
never only filed in a PR body or a document. They do the final review and merge.

**After the operator merges the design branch into main, delete the branches
it used** — the design branch and every ticket branch under it, locally and on
origin — as [the SDLC design](../../../docs/boxes/engineering/sdlc/README.md)
requires. `git branch -d` refuses anything that is not really merged, so it is
its own check; report what was deleted. Then **run the retro**
(`/retro`) in this session, which is the one that holds the run — printed in
chat, autonomous runs included. The retro comes after the merge, not in the
ready-to-merge report: [the SDLC design](../../../docs/boxes/engineering/sdlc/README.md)
places it after the merge, and the operator's final review and merge are stages
it reports on.

## Done when

- Every ticket is merged into the design branch.
- The plan is deleted.
- The design→main pull request is clean at the drift bar.
- The operator has what they need to decide.
- After the operator merges, the retro is printed in chat.
