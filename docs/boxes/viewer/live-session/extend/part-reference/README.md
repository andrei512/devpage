---
page-type: reference
---
# Part reference

The exact form of a [world model part](../README.md#a-world-model-part): the
files of its package, every field of the object `part.ts` returns, what a
function is given when it runs, and the text files.

## The files

<FileTree>

- &lt;part&gt;/
  - extension.yml — the manifest: the part's name and its module
  - part.ts — what the part declares
  - voice.md — optional, text for Voice's prompt
  - fast.md — optional, text for Fast's prompt
  - slow.md — optional, text for the slow agents' prompt
  - &lt;Widget&gt;.tsx — optional, one module for each widget the part declares

</FileTree>

## The manifest

`extension.yml`, as for any
[extension package](../../../extensions/reference/README.md#bundle). Two of
its fields make a part:

- **`id`:** the part's name. Functions are called `<id>.<function>`, and a
  widget reads `useWorld("<id>")`.
- **`world`:** the path of `part.ts` in the package. JavaScript or
  TypeScript.

## `part.ts`

Its default export is a function. It is given `{ z }`, the schema builder
(zod), because a package cannot import the program's packages. It returns one
object. Every field is optional.

```ts
export default ({ z }) => ({ state, entries, functions, widgets, addToTask, showFast });
```

### `state`

A map of field names to schemas. A field's `.default(…)` is its value in a
new session. Every write is checked against the schema, and a write that does
not fit fails the function that made it. The state is kept in
[the store](../../world-model/store/README.md) and survives a restart.

### `entries`

A map of kind names to:

- **`fields`:** a map of field names to schemas.
- **`line(entry)`:** the one line Fast reads of an entry.
- **`wakes`:** `true`, `false`, or `(entry, caller) => boolean`. `false` when
  left out.
- **`image(entry)`:** optional. An image shown to Fast with the line.

### `functions`

A map of function names to:

- **`for`:** `"fast"`, `"slow"`, `"both"` or `"page"`. See
  [Who may call a function](../functions/README.md).
- **`description`:** one line. It is the function's line in Fast's list of
  calls, and a slow agent's description of the tool.
- **`input`:** a map of argument names to schemas. In Fast's script the
  arguments are given in this order.
- **`check(input, shown)`:** optional, for a function Fast may call. It runs
  inside Fast's script, while the script can still fail, with what Fast was
  shown. It throws to refuse the call.
- **`run(input, now)`:** what the function does. What it returns is the
  tool's answer to a slow agent, and what `world.call` resolves to for a
  widget. Fast gets no answer.

### `widgets`

A map of widget names to the [place](../../../on-the-page/widgets/places/README.md)
of each: `"navbar"`, `"panel"` or `"float"`. A widget's module is the file
of its name beside `part.ts`: `Notes` is `Notes.tsx`.

### `now`

What `run` is given beside its input.

- **`now.state`:** the part's state. Changes made to it are saved when `run`
  ends.
- **`now.add(kind, fields)`:** adds an entry of one of the part's kinds.
- **`now.caller`:** who called: `{ who: "fast", handover, messages }`,
  `{ who: "slow", agent, task }` or `{ who: "page" }`.
- **`now.config`:** the `config` given where the part is
  [enabled](../enabling/README.md).
- **`now.follow(async (publish) => …)`:** for work that outlasts the call.
  It starts after `run` is saved. Each `await publish((later) => …)` is a
  step of its own, where `later.state` and `later.add` are as above.

Everything `run` changes and adds is saved in one step, or not at all when it
throws.

### `addToTask(state, task)`

Called when Fast gives work to a slow agent. It returns the lines to add to
the task, or none. `task` holds `prompt`, the text Fast wrote, and what the
message it was made from carried: `page` and `marked` when the `devpage` part
is on.

### `showFast(state)`

Called at each wake-up of Fast. It returns a map of titles to text. Each is
shown to Fast under its title, only when it differs from what Fast was last
shown.

## The text files

`voice.md`, `fast.md` and `slow.md` are Markdown. Each `#` heading starts a
piece.

- **A heading that the core's template for that agent also has:** the piece
  is added to the end of that section.
- **A heading that names a label of the template,** a line that is a name
  and a colon alone, such as `Backend tools:` in Voice's: the piece is added
  at the end of that label's list.
- **Any other heading:** the piece is a new section, after the core's.

The headings of each template are printed by `bun prompt.ts <agent>` in
`live/`. See [The prompts](../../core/prompts/README.md#what-a-part-adds).

## Names

- **In Fast's script:** `<part>.<function>(…)`.
- **As a slow agent's tool:** `<part>_<function>`.
- **From a widget:** `world.call("<part>.<function>", input)`.
- **An entry's kind** is the part's own: two parts may both have a kind named
  `added`.
