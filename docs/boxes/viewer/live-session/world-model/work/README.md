---
page-type: reference
---
# Work

What [the world model](../README.md) holds under the name `work`: the tasks,
their updates and the [slow agents](../../core/slow-agents/README.md) that
hold them. It is the core's own, so every session has it. This page lists its
state, its entry kinds, its functions and its widgets. How they are used is
on [Delegation](../../core/delegation/README.md).

## State

Every task and every current agent. Fast and the side panel both read them
from here.

```js
{ tasks: [...], agents: [...] }
```

## Entry kinds

- **`task`:** a piece of work given to an agent, and how: a new agent, a
  fork, or the same agent. It does not wake Fast.
- **`update`:** what an agent published on a task: `progress`, `question`,
  `finished` or `failed`. It wakes Fast.

The line Fast reads of each is on [One numbered log](../log/README.md#the-kinds).

## Functions for Fast

They give work.

```js
spawn(prompt)        // a new agent, which starts empty
spawn(prompt, route) // the same, on a named route
a2.send(prompt)      // more work for that agent
a2.fork(prompt)      // a new agent that starts with a2's context
```

- **Fast composes each prompt as a string.** Your words go in as a value, so
  Fast writes only what is around them.
- **Each part adds its lines to the prompt** when the call is carried out:
  see [what a task carries](../../core/delegation/README.md#what-a-task-carries).
  A script never writes them.
- **After a hand-over, a script that gives work must use a message of that
  hand-over.** Otherwise it is refused, and the error names the right ids.
  This catches `m14` typed for `m41`.
- What a route is, is on [Routing](../../use/routing/README.md).

## Functions for slow agents

Each harness offers them as tools.

```js
update       // publishes an update on a task
read_log     // reads the session's log of who said and did what
```

- **`update` changes the task and adds its `update` entry in one step.** Only
  the agent that holds the running task may publish on it.
- **`read_log` answers at once,** as the tool's result.

## Widgets

`DevpageLiveTasksButton`, in the `navbar` place, and `DevpageLiveTasks`, in
the `panel` place: the button with the number of running tasks, and the
section of the panel with the agents and their tasks
([the live session's widgets](../../../on-the-page/widgets/system-widgets/README.md#the-live-sessions-widgets)).
