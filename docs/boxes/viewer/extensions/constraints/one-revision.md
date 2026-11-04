# A render uses one extension revision

<Callout kind="tip" title="A review rule, written for the AI reviewer">

This file is a constraint for automated AI code review. On every change to this box or to anything inside it, the
reviewer reads it and checks the diff against the rule below; a change that
breaks it is a finding. People can read it too, but it is written to be
checked: the rule, why it holds, and how a breach shows.

</Callout>

<Callout kind="note" title="The rule">

Every request devpage sends after preparation names the revision it is made against, and a page render keeps the revision current when it began for all of its requests. After a failed capture or preparation, every request fails until a preparation succeeds; the previous revision is never served as current.

</Callout>

## Why

A page could otherwise show added content from one capture beside an embed from another, or show an old model as if it were checked.

## Check

`content` in `core/src/page/content.rs` renders with `Binding::snapshot` copies, not the live bindings. `Binding::call` refuses while `error` is set. The core test removes the input, requires `prepare` and `view` to fail, then restores it and requires recovery. Devpage does not check that a package honors the `revision` it is sent; that is the package's side of the rule.
