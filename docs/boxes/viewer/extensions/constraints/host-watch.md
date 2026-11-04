# Extensions use the host watch

<Callout kind="tip" title="A review rule, written for the AI reviewer">

This file is a constraint for automated AI code review. On every change to this box or to anything inside it, the
reviewer reads it and checks the diff against the rule below; a change that
breaks it is a finding. People can read it too, but it is written to be
checked: the rule, why it holds, and how a breach shows.

</Callout>

<Callout kind="note" title="The rule">

An extension's input is watched by devpage's own watcher, whatever the site's `watch` list says, and its files reach the package only as contents devpage captured. A changed input is prepared again before the change burst's refresh is sent. No extension starts a watcher or polls.

</Callout>

## Why

Otherwise a change can be filtered out before the extension hears of it, or an open page can refresh against the previous preparation.

## Check

The core test in `core/src/extensions.rs` narrows the site's `watch` to one page and still finds a new file under the input in sight, then edits a file and requires a new preparation. In `core/src/watch/mod.rs`, preparation (`extensions::changed`) runs before the burst's `Refresh` is published, and adds at most one.
