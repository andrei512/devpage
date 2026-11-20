---
page-type: reference
---
# Session

What [the world model](../README.md) holds under the name `session`: whether
the voice is on, and what was said. It is the core's own, so every session
has it. This page lists its state, its entry kinds, its functions and its
widget.

## State

Whether the voice is on, and what keeps it from starting.

```js
{ voice: "off" | "starting" | "on", problem: null }
```

In chat mode `voice` stays `"off"`.

## Entry kinds

- **`message`:** something you said or wrote, or something Voice said, with
  its id. It does not wake Fast by itself: the hand-over does.
- **`problem`:** something a script asked for that could not be done. It
  wakes Fast.

A part may add fields to a message at the moment it is stored. The `devpage`
part adds the page it was said on and the part marked then, so "what was
marked when this was said" is answered from the message, not from the state
at the time Fast runs.

The line Fast reads of each is on [One numbered log](../log/README.md#the-kinds).

## Functions

Both are for Fast.

```js
say(text)          // you hear it, or in chat mode read it
restart()          // restarts the session's program; restart(true) also with tasks running
```

- **Only `say` reaches you.** Fast's plain text does not.
- **`say` answers a hand-over.** It runs with the hand-over of the script
  that called it. In realtime mode it fails while the voice is off.
- **`restart` checks first.** It refuses while tasks are running, unless it
  is `restart(true)`, and it never restarts a program whose code does not
  build.

## Widgets

`DevpageLiveOrb`, in the `float` place: the orb that starts and stops the
session and shows the voice and its problem
([the live session's widgets](../../../on-the-page/widgets/system-widgets/README.md#the-live-sessions-widgets)).
