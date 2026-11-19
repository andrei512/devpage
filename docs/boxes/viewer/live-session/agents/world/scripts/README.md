---
page-type: concept
---
# Fast's scripts

How [Fast](../../fast/README.md) acts on [the world](../README.md): it has
one tool, `exec`, which runs JavaScript in a QuickJS sandbox. The ids Fast
sees in its context are the script's variables, and the functions the parts
register for Fast are its calls.

## What a script can use

The ids:

```js
m41                // a message; in a string it is the words
m41.page           // the page address when it was said
m41.marked         // the marked text when it was said; empty when nothing was marked
u12                // an update; in a string it is its text
```

The functions, each on its part's page:

- [Session](../session/README.md#functions): `say`, `restart`.
- [Page](../page/README.md#functions): `page.go`, `page.scroll`,
  `page.panel`, `page.point`, `page.select`.
- [Canvas](../canvas/README.md#functions): `canvas.pad`, `canvas.show`,
  `canvas.close`.
- [Work](../work/README.md#functions): `spawn`, `a2.send`, `a2.fork`.

A function an extension registers for Fast is called the same way
([Who may call a function](../extending/functions/README.md)).

## Examples

You ask a question about a part you marked.

```js
spawn(`The user asked: "${m41}"`);
say("I'm checking that.");
```

You add something to work that is running.

```js
a2.send(`The user adds: "${m44}"`);
```

An agent finished.

```js
say(u12);
```

Five updates arrived at once.

```js
say(`The three wording fixes and the new diagram are in. One question: ${u35}`);
```

You ask to see the part you marked earlier.

```js
page.point(m38.marked);
```

## How a script runs

- **No `await` and no return value.** The calls are collected and carried out
  when the script ends. `exec` answers "ok" or the error.
- **A script that throws does nothing.** No half-done scripts.
- **An unknown id is an error.** Fast gets it back and tries once more.
- **A part's own checks run inside the script,** while it can still fail: a
  quote on the page exactly once and an address from the list of pages
  ([page](../page/README.md#functions)), a pad that exists
  ([canvas](../canvas/README.md#functions)), a message of the hand-over
  ([work](../work/README.md#functions)).
- **A new sandbox for each script,** with 32 MB of memory, stopped after
  300 ms.

The cost of collecting the calls: when a `spawn` fails, Fast learns it at the
next wake-up, as a `problem`, after "I'm checking that" was said.
