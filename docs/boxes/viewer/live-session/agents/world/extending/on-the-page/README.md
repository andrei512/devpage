---
page-type: mechanism
---
# Widgets and state on the page

How a [world part](../README.md#a-world-part) shows on a page: its package
says where its widgets go, and a widget reads the part's state with one call.

## Where the widgets go

- **A package says where its widgets go.** Its `extension.yml` declares its
  widgets, their modules and their
  [places](../../../../../on-the-page/widgets/places/README.md).
- **The panel is a stack of sections.** Each widget placed in `panel` is one
  section. The agents and their tasks are the
  [work part's](../../work/README.md#widgets) section.
- **The session's own widgets are placed the same way,** through its
  manifest, not by name inside devpage.
- **Widgets only.** Pages from a world extension are out of scope.

## Reading a part

```js
const work = useWorld("work")

// on the socket, when a part changes
{ type: "state", part: "work", state: { tasks: [...], agents: [...] } }
```

- **A widget reads a part with one call, `useWorld(part)`.** It returns that
  part's state and draws the widget again when it changes. The
  [`devpage` module](../../../../../on-the-page/widgets/page-module/README.md#useworld-and-world)
  exports it, because a package's widget can import only its own modules.
- **No new connection.** `useWorld` reads from the WebSocket each page already
  holds to the program. Only its `state` message changes; what was heard, an
  agent's session, page actions and the microphone's audio stay as they are.
- **The program sends one part at a time.** A page that connects gets every
  part's state. After that, a change to a part sends that part's state, whole.
  Not a diff: a widget that reads the whole part cannot be wrong after a
  missed message.
- **The live session publishes four parts:**
  [`work`](../../work/README.md#state) for tasks and agents,
  [`canvas`](../../canvas/README.md#state) for the open pad,
  [`page`](../../page/README.md#state) for where you are and what is marked,
  [`session`](../../session/README.md#state) for the voice and its problem.
