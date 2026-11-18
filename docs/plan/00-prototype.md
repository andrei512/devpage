# Prototype: the extendable world

The spike of [The extendable world](../boxes/viewer/live-session/agents/world/extending/README.md).
Measured: 19:56 to 20:05, nine minutes of the thirty. It stopped early
because the rest of the path runs through files other work has open, not
because the time ran out.

## What runs

One path, in new code, with tests (`live/registry.test.ts`, seven tests, and
one in `src/adapters/extensions.rs`):

- A world part registers entry kinds, functions and state (`live/registry.ts`).
- **One function, registered once, is called both ways:** from a script of
  Fast's in the QuickJS sandbox, through a sandbox half generated from the
  function's input, and as a slow agent's tool in the `HarnessTool` shape the
  harnesses already take.
- **State and entries are written in one SQLite transaction**, in the bun
  devpage pins (1.4.4, SQLite 3.51.0). A function that throws writes nothing.
- An entry kind says whether it wakes Fast, and may decide by who called: the
  entry of Fast's own call wakes Fast, a slow agent's does not.
- An extension's module in another folder registers a part with what it is
  handed, the schema builder included.
- A part's whole state is handed out once for each change: what a page would
  be sent.
- **Fast's model reads an image an entry carries.** One request to
  `gpt-6-luna` with a wake-up of one text and one image, a red square made in
  the test; it answered "red". Run with `WORLD_PROTOTYPE_MODEL_CHECK=1`.
- **A package with no library brings widgets**: its `extension.yml` names
  them, and it declares them as a library's `create` reply does.

## What it touched

- `live/`: two new files. Nothing of the running session uses them.
- `src/adapters/extensions.rs`: the manifest's `widgets` and what a package
  with no library declares.

## Where the difficulty was

- **A function's writes cannot be a transaction around its run.** bun's
  SQLite transactions are synchronous and a function is not: running a script
  waits. So a function's `set` and `add` are kept while it runs and committed
  together when it ended. A function reads its own pending writes.
- **Only text crosses into the sandbox**, as expected. The generated half
  checks that required inputs are there and that each has its type. It takes
  its inputs in the order they were registered, which is how Fast's calls are
  written today.
- **A tool's name cannot have the dot** a function's has: `mac.run` is the
  tool `mac_run`.
- Not difficult at all: SQLite, the image, loading a module from another
  folder, widgets from a manifest. The last was one small change, because a
  package with no library already answers `create`.
- An existing test, `a_process_serving`, fails when run from inside a live
  session: it reads `DEVPAGE_PORT` from the environment. Unrelated; the gates
  are green with the session's variables unset.

## What the design got wrong

- "A `set` or an `action` changes state and adds its entries in one step" is
  true only of the writes. What an action does outside the store, a script on
  the computer, has happened by the time the transaction commits and is not
  undone when it fails.
- "Whether it wakes Fast" is not one yes or no for each kind: the computer's
  kind wakes Fast for Fast's call and not for a slow agent's. A kind decides
  with the caller in hand.
- A `get` for both is refused at registration, as designed. Nothing else was
  contradicted.

## What is left

Not touched, because `live/service.ts`, `live/fast.ts` and
`core/assets/widgets/live.ts` have other work's uncommitted changes:

- The registry is beside the session, not under it: the five kinds, Fast's
  calls and the three tools are not moved.
- The core's own checks inside a script: a quote on the page once, an address
  from the list, the hand-over's message used.
- `useWorld` in the page module, and the socket sending one part at a time.
  No widget reads a part in a browser yet.
- A manifest's `places` and its world module; global extensions.
- The migration of `tasks.json`, `agents.json`, `world.jsonl` and the log.
- The extensions reference still says a package without a library declares
  nothing; that page is open in other work too.
- Relative imports between a manifest's widget modules: each widget's module
  is declared alone.
