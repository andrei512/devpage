---
page-type: concept
---
# The session part

The part of [the world](../README.md) named `session`: the voice and what was
said. Fast reads the transcript and cannot change it; what it can do is give
Voice a line to speak.

## State

Whether the voice is on, and what keeps it from starting.

```js
{ voice: "off" | "starting" | "on", problem: null }
```

## Entry kinds

- **`message`**: something you or Voice said, with its id, the page it was
  said on and the part marked then. It does not wake Fast by itself: the
  hand-over does.
- **`problem`**: something a script asked for that could not be done. It
  wakes Fast.

The line Fast reads of each is on [One numbered log](../log/README.md#the-kinds).

## Functions

Both are for Fast.

```js
say(text)          // Voice says it
restart()          // restarts the session's program; restart(true) also with tasks running
```

- **Only `say` is spoken.** Fast's plain text never reaches Voice.
- **`say` answers a hand-over.** It runs with the hand-over of the script
  that called it, and fails while the voice is off.
- **`restart` checks first.** It refuses while tasks are running, unless it
  is `restart(true)`, and it never restarts a program whose code does not
  build.

## Widgets

`DevpageLiveOrb`, in the `float` place: the orb that starts and stops the
session and shows the voice and its problem
([the live session's widgets](../../../../on-the-page/widgets/system-widgets/README.md#the-live-sessions-widgets)).
