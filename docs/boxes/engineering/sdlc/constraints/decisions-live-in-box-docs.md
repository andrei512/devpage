# Decisions live in box docs, not in a decision log

A decision changes how a box works, so it belongs in that subject's focused
design, linked from the box's README. A rule that must not be broken belongs in
the owning box's `constraints/`, or in `docs/constraints/` when system-wide.
The reasoning behind the change goes in the commit message, attached to the
diff it explains.

devpage's docs have no `docs/decisions/`, no ADR directory, and no per-decision
record kept outside the box it concerns.

**Why:** decision logs are write-only. They accumulate, nobody reads them at the
moment a decision is being violated, and agents generate them for anything that
looks like a choice. A rule that review actually checks is worth more than a
record of why the rule was chosen.

**Check:** a diff that adds a decision-log-shaped document — a numbered record,
a dated decision file, a "why we chose X" doc sitting outside a box — fails this.
Ask which box's focused design or applicable constraint it belongs to instead.
