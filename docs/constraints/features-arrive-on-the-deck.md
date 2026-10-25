# A feature a user sees arrives on the deck

<Callout kind="note" title="The rule">

A change that adds or changes something a devpage user sees — a kind of
document, a Markdown feature, a widget, a command, a field, a look — updates
the [showcase deck](../boxes/showcase/README.md) in the same diff: it adds the
slide that proves it, or remakes the slide it changes, and updates the deck's
slide table — and adds or updates its entry in the feature catalog doc. A change that removes a capability removes its slide.

</Callout>

## Why

The deck is the first thing a newcomer sees, and devpage
demonstrating itself. A deck that lags the product undersells it and, worse,
shows things that no longer work. Kept by habit, it lags; kept by rule, every
review asks.

## Check

Review, on every diff. The question is "does a user see something
new or different?" — if yes, the diff touches `docs/talks/devpage/` and the
slide table in the showcase design, and the new or remade slide follows the
deck's rules (shows rather than lists, built with what it shows, fits the
stage in light, dark and PDF). A diff that only changes internals passes
without touching the deck.
