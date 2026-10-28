# Stages

What each stage of the flow does, what it produces and the skill that runs it —
from the design conversation to the retro, and the inverted path a small
change can take. It is a concept page under the [SDLC](../README.md); who may
start each skill is on the [Skills reference](../../skills-reference/README.md).

## Design

A conversation that works out how the system should change. The
operator brings an idea, the two sides debate it, the agent works out the blast
radius, and the loop repeats until a pass turns up nothing unaccounted for. It
produces one thing: an update to the box docs. (`/design`)

## Prototype

A fork spikes the design before anyone plans it, on a branch off
the design branch. Its job is one end-to-end path through whatever the design
newly brings, in real code with tests: fewer ceremonies, not worse work. It has
**thirty minutes of measured time** — read off a clock, never estimated, because
an agent can measure and cannot estimate — and its instruction is to keep going,
so "bigger than I expected" is a reason to continue rather than to report.

If the budget runs out before an end-to-end path exists, the design update is too
big. That is evidence rather than an opinion, and the split goes back to the
design conversation rather than being the prototyper's call.

The branch it leaves behind is the implementation branch: work is hardened in
place, and it is thrown away only when the approach proved wrong. Its report is
the plan's first draft. (`/prototype`)

## Plan

A separate session that turns the prototype into tickets, which is
mostly deciding what it takes to productionise what already runs. Because the
work has been measured rather than imagined, plans come out proportional — two
or three tickets, not nine. The operator approves a story — one line per step —
not a list of tickets. (`/plan`)

## Build

A coordinator hands **lanes** to forks, each in its own worktree and
on its own branch off the design branch, and merges them one at a time as they
pass review. A lane is a related chain of tickets given to one fork, which
starts working rather than reading itself in. Parallelism comes from running
several lanes. The coordinator never writes code itself. (`/build`, with lanes
running `implement-ticket`)

A fork carries the conversation that produced the design only when `/build` runs
in the session that had it, which the stage split makes unusual — plan is a
separate session, so build is normally two stages downstream. Assume it doesn't:
the brief carries what was decided.

## Review

Every change is checked against the bar its target sets. A ticket
is reviewed by the fork that built it, at the task bar, and comes back once,
converged; the coordinator merges it. Fresh eyes come once, at the end: the
design branch's pull request to main is reviewed at the drift bar by an agent
that neither wrote nor coordinated the branch, its findings come back as new
tickets, and the operator does the final review and merge. A finding is
something reachable by accident — what only deliberate evasion could trigger is
a note, not a round. (`/review`)

## Release

The operator's call, from main. A release publishes the version in
`Cargo.toml` to [crates.io](https://crates.io), from main's tip with the gates
green, and tags that commit `v<version>`. devpage depends on its core,
`devpage-core`, which depends on the design engine, `devpage-design`; both
carry the same version and are published first, in that order —
`cargo publish -p devpage-design`, `cargo publish -p devpage-core`, then
`cargo publish -p devpage` — since crates.io resolves each crate's
dependencies from the registry. A version is published once, so a release
moves the three crates past the last published version together. Each release adds its entry to
[`CHANGELOG.md`](../../../../../CHANGELOG.md) — what changed for a user, newest
first — in the commit that moves the version. Publishing is permanent — a version can be yanked but never
replaced — so it happens only when the operator says so. The
[example image](../../../viewer/deploying/README.md#the-image) installs the
released version, so it follows each release.

## Retro

After the operator merges the design branch into main, the session
that coordinated the build reports where the time went and what each stage
caught, in chat, changing nothing. It is not part of the ready-to-merge report.
The operator carries what repeats across runs and says when a rule changes.
(`/retro`)

## Hack

A small change can run the flow inverted: `/hack` aligns on the change, builds
it with the operator watching, and writes the docs once the operator says it's
right. It ends at the same bar as any other branch.
