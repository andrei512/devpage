# No knowable rework

<Callout kind="note" title="The rule">

Build the final shape of what's needed now. A change is wrong if, on the day
it lands, the rework is already nameable — nothing about the requirements has
to change for it to need redoing or removing. One test, two failure modes:

- **A corner-cut fails forward**: the redo is scheduled the day it ships.
  Rendering Mermaid by hand-written string replacement when the vision says
  extensions plug into the parser; a full rebuild on every save when the vision
  says rendering is realtime. The tell is a known future task created to save
  present effort — "do X properly later" where later is guaranteed.
- **Overengineering fails backward**: the removal is justified the day it
  ships. A layout no document uses, a plugin system for imagined extensions, a
  config field nothing reads.

"Needed now" is judged against the design and the vision for the shape of
what is built, not against the current ticket alone; whether to add new
surface at all is governed by
[Surface area arrives with its consumer](surface-arrives-with-its-consumer.md).
And "final shape" is not
"every feature" — a syntax set holding only what today's documents use is
final-shaped, because adding an extension when a document needs one is
ordinary evolution, not scheduled rework.

</Callout>

## Why

Cut corners compound — every one is a migration someone must
remember, and the system's docs stop being true the moment "temporarily wrong"
ships. Overengineering compounds too: unconsumed surface is maintenance,
drift risk, and a contract every user's documents must keep honoring. Both are
the same mistake — spending on a shape the present doesn't ask for — and naming
them as one rule keeps "do it right" from being heard as "build it big."

## Check

Review, on every diff and every design. The question is "what
rework is already knowable?" — if the answer names a redo, it's a corner; if
it names a removal, it's overengineering; if it names nothing, the size is
right. In a design conversation the recommendation must be the shape that
passes this test; a smaller shape may appear only as a named cost, never as
the proposal.
