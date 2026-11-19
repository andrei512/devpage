---
page-type: concept
---
# The work part

The part of [the world](../README.md) named `work`: the tasks, their updates
and the [slow agents](../../slow-agents/README.md) that hold them. Fast reads
it and gives work to an agent; an agent publishes its updates into it.

## State

Every task and every current agent. Fast and the side panel both read them
from here.

```js
{ tasks: [...], agents: [...] }
```

## Entry kinds

- **`task`**: a piece of work given to an agent, and how: a new agent, a
  fork, or the same agent. It does not wake Fast.
- **`update`**: what an agent published on a task: `progress`, `question`,
  `finished` or `failed`. It wakes Fast.

The line Fast reads of each is on [One numbered log](../log/README.md#the-kinds).

## Functions

Three are for Fast: they give work.

```js
spawn(prompt)        // a new agent, which starts empty
spawn(prompt, route) // the same, on a named route
a2.send(prompt)      // more work for that agent
a2.fork(prompt)      // a new agent that starts with a2's context
```

- **Fast composes each prompt as a string.** Your words go in as a value, so
  Fast writes only what is around them. The page you were on and the part you
  had marked are added to every prompt by the program, so a script never
  writes them.
- **After a hand-over, a script that gives work must use a message of that
  hand-over.** Otherwise it is refused, and the error names the right ids.
  This catches `m14` typed for `m41`.
- What a route is, is on [Routing](../../routing/README.md).

Three are for slow agents: each harness offers them as tools.

```js
update       // a set: publishes an update on a task
write_comment // an action: saves a comment on the part marked when the task was given
read_log     // a get: reads the session's log of who said and did what
```

- **`update` changes the task and adds its `update` entry in one step.** Only
  the agent that holds the running task may publish on it.
- **`write_comment` is used only when you ask for a comment,** and writes a
  devpage [comment](../../../../share/comments/README.md).

## Widgets

`DevpageLiveTasksButton`, in the `navbar` place, and `DevpageLiveTasks`, in
the `panel` place: the button with the number of running tasks, and the
section of the panel with the agents and their tasks
([the live session's widgets](../../../../on-the-page/widgets/system-widgets/README.md#the-live-sessions-widgets)).
