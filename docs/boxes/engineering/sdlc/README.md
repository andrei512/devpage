# SDLC

How work gets from an idea to running code: the stages every change to devpage
goes through and the skill that runs each, the branches that carry a design
until the code catches up, the two review bars, and how the box docs are
organized. It is a concept page of the [Engineering](../README.md) box;
[Running the flow with skills](../running-the-flow/README.md) is the short
version, from the operator's keyboard.

```mermaid
flowchart LR
  design["Design"] --> prototype["Prototype"]
  prototype --> plan["Plan"]
  plan --> build["Build"]
  build --> review["Review"]
  review --> release["Release"]
  click design href "./stages/README.md#design" "Design"
  click prototype href "./stages/README.md#prototype" "Prototype"
  click plan href "./stages/README.md#plan" "Plan"
  click build href "./stages/README.md#build" "Build"
  click review href "./stages/README.md#review" "Review"
  click release href "./stages/README.md#release" "Release"
```

Review findings go back to Build as new tickets; after the merge, the
[retro](./stages/README.md#retro) reports where the time went. A small change
can run the flow inverted, as a [hack](./stages/README.md#hack).

<Cards columns={3}>

- [Design](stages/README.md#design) — `/design`: a conversation, then the box docs on `design/<name>`.
- [Prototype](stages/README.md#prototype) — `/prototype`: thirty measured minutes, one end-to-end path.
- [Plan](stages/README.md#plan) — `/plan`: the prototype turned into a story of tickets.
- [Build](stages/README.md#build) — `/build`: lanes in their own worktrees, merged one at a time.
- [Review](stages/README.md#review) — `/review`: the task bar per ticket, the drift bar at the end.
- [Release](stages/README.md#release) — The operator's call: `cargo publish` and a tag.

</Cards>

## Boxes and documentation

How box docs are organized and kept true — one home per subject, constraints
one file per rule, docs on main describing the system as it is — is on the
[system catalog](../../README.md#how-to-read-the-catalog).

## Constraints

The SDLC's two rules are listed under [Topics](#topics), one line each, and
reviewed on every change to the process or to box docs. System-wide rules are
the [general constraints](../../../constraints/README.md).

## Topics

- [Stages](stages/README.md) — what each stage does and produces, from design
  to retro, and the hack.
- [Branches](branches/README.md) — design, prototype, ticket and direct
  branches: how they merge and when they are deleted.
- [Review bars and CI](review-bars/README.md) — the task bar and the drift
  bar, and what checks them today.

### Constraints

- [On main, the box docs describe the system as it is](constraints/docs-are-true-on-main.md)
  — anything merged to main leaves the docs an accurate description of the
  code; a design not yet built waits on its design branch.
- [Decisions live in box docs, not in a decision log](constraints/decisions-live-in-box-docs.md)
  — a decision goes into the owning box's docs or constraints and its reason
  into the commit message; there is no ADR directory.
