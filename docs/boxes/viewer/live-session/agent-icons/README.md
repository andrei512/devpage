---
page-type: concept
---
# Agent icons

The icon that stands for one slow agent in the [live session](../README.md)
is a faint sphere with plasma inside that does not fill it. The session's own
orb is drawn the same way. An agent's orb is beside its name in
[the side panel](../design/README.md#the-side-panel) and on top of its chat.

## Why

The session already has one orb, in the corner of every page. An agent's icon
is its small sibling: the side panel shows the agents, each with a name taken
from what it works on, and its orb beside the name.

The sphere is mostly empty on purpose, and the plasma moves inside it. It is
not a full dark sphere with colour on it.

## The faint sphere

No dark glass: the whole sphere has a very light tint of the agent's colour,
and a contour of that colour that grows toward the edge. The page shows
through. Inside it, one of five kinds of plasma:

| Kind | What the plasma is |
|---|---|
| **Cloud** | One soft mass, denser in the middle. |
| **Filaments** | Thin bright threads that cross. |
| **Ribbons** | Wide bands that bend through the sphere. |
| **Swirl** | Arms that turn around the centre, one or two. |
| **Ring** | A band around the centre, with the middle left empty. |

An orb is the same picture at every size: beside a name in the panel it is
small, on top of a chat larger. It moves while its agent works.

## The session's orb and the agents' orbs

The session's orb is the same kind of orb: a faint sphere with plasma inside.

| The session's orb | What it means |
|---|---|
| Purple plasma | The session is on and listening. Purple is red and blue mixed: the orb rests between the two voices. |
| Red | You are speaking. |
| Blue | The assistant is speaking. |
| An empty grey sphere, only its contour | The session is off. When it starts, the purple plasma fades in. |
| A ring around it, grey or purple | Work is running. The ring never takes an agent's colour. |

**An agent's orb may have any colour,** purple, red and blue too. The
session's orb is told apart by where it is, in the corner of the page, not by
its colour.

## How an orb is made

- **The seed** is the agent's number, not its name. The name can change while
  the agent works; the orb must not.
- **The seed picks** the kind of plasma, two colours, any colours, where in
  the noise the plasma lives, how much of the sphere it takes, and how fast it
  moves.
- **One small shader draws them all.** It is the session orb's own noise, cut
  down. A browser allows only a few shader canvases on one page, so one hidden
  canvas draws every orb in turn and copies it to a plain canvas.
- **The state is on the orb.** The plasma moves while the agent works and
  rests when it is idle, when the orb is also faint; it flares when the agent
  has a question. See [What the orb says](../design/README.md#what-the-orb-says).
- **A fork does not look like its parent.** It has its own number, so its own
  orb.

The code is `core/assets/widgets/live-agent-orb.tsx` for an agent's orb and
`core/assets/widgets/live-orb.ts` for the session's.
