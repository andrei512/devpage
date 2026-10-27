# Running the flow with skills

How the operator drives a change through devpage's development flow in this
repository: the slash command that starts each stage, which skills only the
operator can start and which agents reach for themselves, where each stage's
output lands, and how to choose between the full flow, a hacking session and a
direct pull request. It is the essentials article under
[Engineering](../README.md); the [SDLC](../sdlc/README.md) is the full design
of the process, and the [skills reference](../skills-reference/README.md) lists
every skill exactly.

## The mental model

**Each stage is a skill, and the operator starts it.** A skill is a folder
under `.claude/skills/` whose `SKILL.md` tells an agent how to run one kind of
work. The operator types its name as a slash command — `/design`, `/plan` — and
the agent follows it. Between stages nothing moves on its own: a settled design
is not permission to spike it, and a conversation stays a conversation until
the operator asks for the artifact (`AGENTS.md`).

**Some skills only the operator can start**; agents reach for the rest when
their work calls for one — the design session forks itself to run
`prototype`, a lane runs `implement-ticket`, every stage uses `align` to talk.
Which is which is listed once, on the
[skills reference](../skills-reference/README.md#the-skills).

**The branches carry the state.** A design lives on `design/<name>` until the
code catches up; tickets are branches off it; which tickets are done is which
branches are merged. `main` only ever holds docs that are true.

## What the operator types

The full flow, step by step, with an imagined change — the sidebar collapsing
its groups — as the example. It illustrates the flow; it isn't a planned
feature.

<Steps>

1. Design
   An idea that changes what the box docs say. A conversation, using `align`,
   until a blast-radius pass turns up nothing new; then, when asked, the write:
   the box docs under `docs/boxes/` (and `docs/constraints/` for a system-wide
   rule), on a branch `design/<name>` cut once the shape is clear.

   $ /design

   *Example:* the agent works out the blast radius — the viewer's navigation
   pages, `assets/devpage.js`, the stylesheet — and writes the change into
   `docs/boxes/viewer/` on `design/collapsible-groups`.

2. Prototype
   After the design is settled, the operator asks for the spike. The design
   session forks itself; the fork builds one end-to-end path in thirty measured
   minutes, with tests, on `prototype/<name>`, commits its report at
   `docs/plan/00-prototype.md` and merges back into the design branch.

   $ /prototype

   *Example:* one group collapses end to end with a test.

3. Plan
   After the prototype has merged. A separate session turns the report into
   tickets; the operator approves a story, one line per step. One file per
   ticket lands in `docs/plan/`, numbered in dependency order, each opening
   with front-matter (`id`, `blocked-by`, `boxes`).

   $ /plan

   *Example:* a two-step story; `docs/plan/01-…md` and `02-…md` appear.

4. Build
   After the story is approved. A coordinator hands lanes to forks, each in its
   own worktree running `implement-ticket`, and merges them with `--no-ff` as
   they pass their own review; then deletes `docs/plan/` and opens a pull
   request from the design branch to `main`.

   $ /build

5. Review
   Any branch or pull request — by the operator, a lane or the coordinator —
   is checked against its target's bar: the task bar into a design branch, the
   drift bar into `main`. On the design branch's pull request a fresh agent
   reviews, and findings come back as new tickets.

   $ /review

6. Merge, then retro
   The operator does the final review and the merge into `main`; `/build`'s
   session deletes the design branch and its ticket branches, then reports one
   line per stage — cost and catch — and a paragraph on what looked off, in
   chat only.

   $ /retro

</Steps>

A bug fix or a small change can go another way: `/hack` aligns, then builds
together in the operator's checkout; docs, tests and gates once the operator
says it's right, on a branch `hack/<name>` with a pull request to `main`.

<Callout kind="note" title="Plan files in the served docs">

`docs/plan/` sits inside this documentation's folder, so while a design branch
holds it, devpage serves those files like any page — `/docs/plan/00-prototype.md`
renders, and saving one refreshes open pages. No Topics section lists them, so
they never appear in the sidebar; open them by address. They never reach
`main`: `/build` deletes `docs/plan/` before the pull request opens.

</Callout>

## Choosing a path

<Cards columns={3}>

- [Full flow](../sdlc/stages/README.md) — The change alters what the box docs say: a new mechanism, a new contract, a rule. Design → prototype → plan → build → review.
- [Hacking session](../sdlc/stages/README.md#hack) — A bug fix or small change where iterating on running code beats writing docs first. Only the operator opens one; if it crosses boxes or bends a constraint, it goes back to `/design`. It ends at the same bar: docs true, tests, gates, a reviewed pull request.
- [Direct pull request](../sdlc/branches/README.md#changes-that-alter-no-design) — The change alters no design: a typo, a dependency bump, a fix whose shape the docs already describe. Branch off `main`, pull request, task bar. No skill is required.

</Cards>

## Where next

- [SDLC](../sdlc/README.md) — the reasons behind each stage, the branch
  topology and the review bars.
- [Skills reference](../skills-reference/README.md) — all twelve skills: who
  starts each, what goes in and what comes out.
- [Developing devpage](../developing-devpage/README.md) — building, testing
  and the gates.
