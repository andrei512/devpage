---
name: hack
description: A hacking session — prototype first, design after. For bug fixes and small changes where iterating on running code beats writing docs first. The operator opens it explicitly, or you offer it when a request sounds small.
disable-model-invocation: true
---

# Hack

The design flow inverted for small work: align on the change, build it
together, and only when the operator says it's right does the design get
written. The bar at the end is identical — docs true, tests pinning the
behavior, gates green, a reviewed PR. What moves is the order: for a bug
fix or a small change, the code is the design conversation, and making the
operator sit through a doc-first pipeline is how small things take a day.

## Entering

Three doors:

- The operator says so — "hack this", "quick change", "let's just fix it".
  That's the mode; go.
- A bug report, or a request that sounds small in scope. If it's clearly
  small, offer the mode in one line; if you can't tell whether they want a
  hacking session or a design session, use the [align skill](../align/SKILL.md)
  to settle the intended mode.
- Never infer it silently. A hacking session is the operator's explicit
  trade of ceremony for speed, and only they can make it.

## Align first, then branch

Read and use the [align skill](../align/SKILL.md) to settle what is wrong,
what "fixed" looks like, and what is out of scope. For a bug that means the
symptom and expected behavior; for a change it means the shape. Once the
operator considers alignment sufficient, cut `hack/<name>`.

The session runs in the operator's checkout — they are looking at the
running thing, and that is the point. The mode is their standing
permission under the checkout rule; when the session ends, the checkout
goes back where it was.

## The loop

Prototype first, iterate together. You edit directly — no lanes, no
tickets, no red-first, no full gates per step. The operator looks, reacts,
redirects; use `align` to discuss their feedback and any change in direction.
The only bar inside the loop: it compiles and the tests you touched pass. Say
what you changed in a line, not a report.

**The escape hatch:** if the hack starts crossing boxes, bending a
constraint, or growing past what was aligned — stop and say so. "This
outgrew hacking" hands it to the design conversation; it must never grow
silently into an unreviewed redesign.

## When the operator says it's done

The debt is paid, in one pass, before anything merges:

1. **Docs** — update every box doc and constraint the change touched, so
   the docs describe the system as built (`docs-are-true-on-main`).
2. **Tests** — pin what the hack built. A bug fix gets its red-then-green
   after the fact: confirm the test fails without the fix, passes with it.
   **Mutations use file copies (`cp`), never `git checkout --` or `git
   restore`** — the session's work is uncommitted while the debt is being
   paid, and git-restoring a file you just mutated wipes the session's
   edits with it, leaving them to be rebuilt from the conversation.
3. **Gates** — the full set, same as any branch.
4. **Self-review at the task bar**, then the normal flow: PR, CI, merge on
   green.

A hacking session never merges mid-loop, and its PR meets the same bar as
a design branch's. The inversion buys speed on the way in, never a
discount on the way out.
