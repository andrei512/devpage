---
page-type: concept
---
# One numbered log

Every change to [the world model](../README.md) is one entry with a number
and a time, in one log that is only added to. This page says what the kinds
of entry are, what the numbering gives, and how much of the log Fast holds.

## The kinds

Each kind has an owner, which says the kind's fields, the line Fast reads of
an entry, and whether an entry wakes Fast. The core and the two parts that
come with the session bring six kinds. A part
[you add](../../extend/README.md) brings its own.

- **`message`** ([session](../session/README.md)): something you said or
  wrote, or Voice said, with its id, and from the `devpage` part the page it
  was said on and the part marked then.

  ```
  m41  user  "Why is this step here?"  page /docs/waiting-list/README.md  marked "An offer stays open"
  ```

- **`problem`** ([session](../session/README.md)): something a script asked
  for that could not be done.

  ```
  problem  devpage.point could not be done: …
  ```

- **`task`** ([work](../work/README.md)): a piece of work given to an agent,
  and how: a new agent, a fork, or the same agent.

  ```
  t7  given to a2 (a new agent)  "…"
  ```

- **`update`** ([work](../work/README.md)): what an agent published on a
  task: `progress`, `question`, `finished` or `failed`.

  ```
  u12  t7 a2  finished  "The page now says seven days."
  ```

- **`page`** ([devpage](../devpage/README.md)): what happened on the page, by
  you or by Fast, as one sentence.

  ```
  page  The user marked "An offer stays open".
  ```

- **`canvas`** ([canvas](../canvas/README.md)): the canvas was opened or
  closed, by you or by Fast.

  ```
  canvas  The user closed the canvas.
  ```

## What wakes Fast

A hand-over wakes Fast. For an entry, its kind decides: of the six above,
`update` and `problem` wake it.

## What the numbering gives

- "What was marked when this was said" has an exact answer: the mark at the
  message's time, not at the time Fast runs.
- "Everything since entry 412" has an exact answer. That is what a wake-up
  carries, and what a [swap](../../core/fast/README.md#swapping-fast-before-its-context-fills)
  adds.
- Routing can be tuned later, because every decision is in the log with what
  Fast saw.

[The store](../store/README.md) holds the full stream for the
project, in `world.sqlite`. The session keeps the newest 2,000 entries in
memory. Message and update ids continue across restarts.

## What Fast holds

Fast holds a window of the world model: the recent messages, the page as it is now,
the recent tasks and their agents. Everything in it has an id: `m41` is a
message, `u12` an update, `a2` an agent.

What falls out of the window is forgotten. Fast cannot query older history,
because a lookup would add lag. When it cannot place what you said, it starts
a new agent.
