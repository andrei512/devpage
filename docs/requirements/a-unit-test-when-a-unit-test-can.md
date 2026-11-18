---
page-type: review-rule
---
# A test must be a unit test when a unit test can check the same thing

<Callout kind="tip" title="A review rule, written for the AI reviewer">

This file is a requirement for automated AI code review. On every change, whatever it touches, the
reviewer reads it and checks the diff against the rule below; a change that
breaks it is a finding. People can read it too, but it is written to be
checked: the rule, why it holds, and how a breach shows.

</Callout>

<Callout kind="note" title="The rule">

A test must be a unit test when a unit test can check the same thing. A unit
test calls one piece of the code directly, in memory. An integration test
puts several pieces together or starts something outside the code: a server,
a browser, a container, another program, the network.

</Callout>

## Why

A unit test runs in milliseconds and fails at the piece that is wrong. An
integration test is slower and fails far from the cause. Every one that a
unit test could replace makes `devpage check` slower for nothing, and a slow
check is run less often.

## Check

For each test the change adds, ask what it asserts and which piece of code
decides that. If one piece decides it, the test calls that piece; a test that
goes through more than it needs is a finding. An integration test is right
only for what no single piece decides: that the pieces fit together, or what
only a browser, a container or the network can show.
