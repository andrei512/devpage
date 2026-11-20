---
page-type: concept
---
# Fast's scripts

[Fast](../../core/fast/README.md) changes [the world model](../README.md) in
one way: it writes a short script. This page says what a script can use,
shows the common ones, and says how a script is run. Fast has one tool,
`exec`, which runs JavaScript in a QuickJS sandbox. The ids Fast was shown
are the script's variables, and the functions declared for Fast are its
calls.

## What a script can use

The ids:

```js
m41                // a message; in a string it is the words
m41.page           // the page address when it was said
m41.marked         // the marked text when it was said; empty when nothing was marked
u12                // an update; in a string it is its text
```

The functions, each on its owner's page:

- [Session](../session/README.md#functions): `say`, `restart`.
- [Work](../work/README.md#functions-for-fast): `spawn`, `a2.send`,
  `a2.fork`.
- [The devpage part](../devpage/README.md#functions-for-fast): `devpage.go`,
  `devpage.scroll`, `devpage.panel`, `devpage.point`, `devpage.select`.
- [The canvas part](../canvas/README.md#functions): `canvas.pad`,
  `canvas.show`, `canvas.close`.

The core's functions are called by their bare names. A part's function is
called as `<part>.<function>`, so a part
[you add](../../extend/functions/README.md) named `notes` with a function
`add` is called `notes.add(…)`.

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
devpage.point(m38.marked);
```

## How a script runs

- **No `await` and no return value.** The calls are collected and carried out
  when the script ends. `exec` answers "ok" or the error.
- **A script that throws does nothing.** No half-done scripts.
- **An unknown id is an error.** Fast gets it back and tries once more.
- **A function's input is checked inside the script,** while it can still
  fail, and so are a function's own checks: a quote on the page exactly once
  and an address from the list of pages
  ([devpage](../devpage/README.md#functions-for-fast)), a pad that exists
  ([canvas](../canvas/README.md#functions)), a message of the hand-over
  ([work](../work/README.md#functions-for-fast)).
- **A new sandbox for each script,** with 32 MB of memory, stopped after
  300 ms.

The cost of collecting the calls: when a `spawn` fails, Fast learns it at the
next wake-up, as a `problem`, after "I'm checking that" was said.
