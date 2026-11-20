---
page-type: concept
---
# Design

What the [live session](../../README.md) looks like: the orb, and the screens of
one request from start to end.

## The screens

All the screens are on one board. Open it, zoom and pan, click a screen to
open it, and click the orb there to step to the next one. In the screens with
a done task, clicking the done task goes to screen 8.

![The screens](screens.design)

## The orb

The orb sits in the bottom right corner of every page. Pressing it starts or
stops the session. It is a faint sphere with plasma inside, drawn with a
shader, and it shows what the session is doing. Why it looks like this is on
[Agent icons](../agent-icons/README.md#the-sessions-orb-and-the-agents-orbs).

- **Off:** An empty grey sphere: only its contour, no plasma.
- **Starting:** Purple plasma fades in and spins fast.
- **On, listening:** Purple plasma moves inside the sphere and does not fill it.
- **You talk:** The plasma turns red, grows and gets turbulent: the louder, the faster.
- **The assistant talks:** The same, in blue. Only the colour differs.
- **Work is running:** A ring turns around it, grey when the session is off and purple when it is on. It can be on in any state.

When something is wrong, a short line beside the orb says what: the key is
not set, the microphone did not open, or the browser wants a click before it
plays sound ("Click anywhere to hear me").

## The side panel

The panel lists agents, each with the tasks it has.

![The side panel with agents and their tasks](agents-panel.design)

- **The button.** A list button in the navbar opens the panel. It shows how
  many tasks are running.
- **On top, the last thing heard or said,** in a box two lines high, as high
  as the navbar. Its height is fixed, so the list under it does not move when
  the text changes. Of longer text, the end shows.
- **Two groups,** each under a small heading with a thin line between them:
  "Working", the agents that work or have a question, and "Earlier", the idle
  ones. In each, the one touched last is on top.
- **An agent row** has the agent's orb and a name taken from what it works
  on. It does not show the harness.
- **A working agent is open,** and it stays open for five minutes after it is
  done. Its tasks are under it, one row per task: a turning arc while it runs,
  a check when it is done, then its title on one line, cut with three dots
  when it is longer, and nothing under it. Clicking a done task goes to what
  it changed.
- **Only its recent tasks,** newest first. An agent shows at most three: the
  ones that run, and the ones that ended in the last five minutes. When none
  is that recent, it shows its last one.
- **"Show more"** under an agent's tasks shows all of them, and "Show less"
  hides them again.
- **An agent idle for more than five minutes is one row,** with its last task
  under its name. The small arrow at the end of its row opens its tasks, and
  folds an open agent.
- **A closed agent is not in the panel.** Its work is on disk, in the files
  and the comments.
- **A fork is its own agent,** with its own row. The panel has two levels
  only: agents and tasks.

### An agent's name

- **The agent names itself,** in its first update, when it also gives its
  task a title. It is the one that knows what the work is.
- **Until then** its row shows the title of its first task.
- **It may rename itself** on a later task, when its scope has changed.
- **The agent is its number, not its name.** Its tasks and its orb hang on
  the number, so a rename changes only the text on its row.

### What the orb says

The orb carries the agent's state. The task rows keep only the arc and the
check.

- **Working:** The plasma moves, in full colour.
- **Idle:** The plasma rests, and the orb is faint.
- **Has a question for you:** The orb flares: it grows and brightens, again and again.

How an orb is drawn is on [Agent icons](../agent-icons/README.md).

## An agent's session

The side panel is the quick look. To see what an agent is really doing, click
its name.

![An agent's chat, open in the panel's place](agent-session.design)

- **In.** A click on an agent's name in the side panel opens its chat. The
  list of agents is gone, and the chat takes its place.
- **Twice as wide.** The chat is twice the width of the side panel, about the
  size an agent has in a code editor.
- **The page stays.** It is narrower, and you still see it and can click a
  part of it.
- **Out.** The arrow at the top left of the chat goes back to the list of
  agents.
- **Folding an agent's tasks** in the list is the small arrow at the end of
  its row, since a click on the name opens the chat.
- **On top,** the agent's orb and its name. That is how you know which agent
  you are talking to.
- **Only its session, as it runs now.** One conversation: what you said, its
  work, what it answered. No list of tasks and no task headings: those are in
  the side panel.
- **The work is folded** to one line that says how many steps it took and
  what it changed. A click unfolds it. What it is doing now is shown as
  steps, with the newest one last.
- **The same shape for every harness.** You see messages and steps, not the
  harness's own screen. What they are made of is on
  [The harness](../../core/harness/README.md).
- **A chat box at the bottom,** as in any app for chatting with an agent.
  What you type there is sent to this agent as a message. Enter sends it;
  Shift and Enter is a new line.
- **Your voice still goes where Fast sends it,** also while a chat is open.
- **The corner orb does not change.** Red, blue and purple still mean who is
  speaking. It does not take the agent's colours.

## Scratch pads

Some ideas cannot be said well by voice. Then the assistant shows you a
canvas: a page that is not in the project, where you talk and develop the
idea together, shape the plan and make it visual, until you mean the same
thing. How it is built is on [The scratch pad](../../world-model/canvas/README.md).

![The canvas, filling the whole screen](scratch-pad.design)

- **One canvas.** You see one idea at a time, the one you are talking about.
- **It fills the whole screen.** It is on top of everything and covers the
  page and both sidebars. All that shows around it is a thin gray edge. It
  has rounded corners and no shadow.
- **The pad stays a column in the middle.** A wide screen gives it more air,
  not longer lines.
- **It is drawn like a page.** Markdown, widgets and diagrams, the same way
  as any devpage page. You can click a part of it and talk about that part.
- **The assistant writes it.** You talk, and the canvas changes in front of
  you.
- **The pads are the assistant's business.** Behind the canvas there is one
  scratch pad per idea. The assistant makes them, names them and picks the
  one to show. You do not switch between them: there are no tabs and no list.
- **When you move to another idea, the canvas shows it.** The assistant
  changes the pad because of what you said, and says which idea it is
  showing.
- **The cross is in the top right corner.** It closes the canvas, and the
  page is there as you left it.
- **The side panel is on the canvas, as a card.** The same panel as on a
  page, with the same inside, in a rounded container with a border, under the
  cross. On a page it stays flat against the right edge.
- **The orb stays on top,** so you can still stop the session.
