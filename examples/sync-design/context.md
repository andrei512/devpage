# Context

The app loses writes made without a connection. This doc proposes an outbox
that keeps them until the server acknowledges them; the
[rollout](rollout.md#steps) ships it in three steps.

## Risks

A write replayed twice must not apply twice.
