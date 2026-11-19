---
page-type: concept
---
# One numbered log

Every change to [the world](../README.md) is one entry with a number and a
time, in one log that is only added to. This page says what the kinds of
entry are, what the numbering gives, and how much of the log Fast holds.

## The kinds

Each kind belongs to a part. The four bundled parts bring five kinds, and a
part an [extension](../extending/README.md) registers brings its own.

| Kind | Part | What it is | The line Fast reads |
|---|---|---|---|
| `message` | [session](../session/README.md) | Something you or Voice said, with its id, the page it was said on and the part marked then | `m41  user  "Why is this step here?"  page /docs/waiting-list/README.md  marked "An offer stays open"` |
| `problem` | [session](../session/README.md) | Something a script asked for that could not be done | `problem  page.point could not be done: …` |
| `page` | [page](../page/README.md) | What happened on the page, by you or by Fast, as one sentence | `page  The user marked "An offer stays open".` |
| `task` | [work](../work/README.md) | A piece of work given to an agent, and how: a new agent, a fork, or the same agent | `t7  given to a2 (a new agent)  "…"` |
| `update` | [work](../work/README.md) | What an agent published on a task: `progress`, `question`, `finished` or `failed` | `u12  t7 a2  finished  "The page now says seven days."` |

## What wakes Fast

A hand-over wakes Fast. For an entry, its kind decides: of the five above,
`update` and `problem` wake it.

## What the numbering gives

- "What was marked when this was said" has an exact answer: the mark at the
  message's time, not at the time Fast runs.
- "Everything since entry 412" has an exact answer. That is what a wake-up
  carries, and what a [swap](../../fast/README.md#swapping-fast-before-its-context-fills)
  adds.
- Routing can be tuned later, because every decision is in the log with what
  Fast saw.

[The store](../extending/store/README.md) holds the full stream for the
project, in `world.sqlite`. The session keeps the newest 2,000 core entries
in memory. Message and update ids continue across restarts.

## What Fast holds

Fast holds a window of the world: the recent messages, the page as it is now,
the recent tasks and their agents. Everything in it has an id: `m41` is a
message, `u12` an update, `a2` an agent.

What falls out of the window is forgotten. Fast cannot query older history,
because a lookup would add lag. When it cannot place what you said, it starts
a new agent.
