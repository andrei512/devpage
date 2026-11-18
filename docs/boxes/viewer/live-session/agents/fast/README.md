---
page-type: mechanism
---
# Fast, the hub

Fast routes in two directions: what you say to the slow agents, and what they
report back to you. It does no work itself. This page also says how it picks
an agent, and how it is replaced before its context fills. The code is
`live/fast.ts`; the page belongs to [the agents](../README.md).

```mermaid
flowchart LR
  voice["Voice"] -->|"hand-over"| loop
  slow["Slow agents"] -->|"updates"| loop
  loop["Fast's loop<br/>wakes Fast"] --> fast["Fast<br/>writes one script"]
  fast -->|"exec"| loop
  loop -->|"say"| voice
  loop -->|"task, follow-up"| slow
  loop -->|"go, scroll, point"| page["The page"]
```

## What wakes Fast

Two things, and nothing else:

- **A hand-over from Voice.** Fast gets the new messages, with ids. It is
  woken at once.
- **Updates from slow agents,** and problems: calls a script asked for that
  could not be carried out. They are collected for 400 ms, so several that
  arrive together are one wake-up.

Either way it also gets the world changes since its last run. Fast runs one
wake-up at a time. What arrives while it runs is delivered together in the
next one.

Fast runs only while the voice is on. What the agents published while it was
off, it gets when the voice starts.

### What a wake-up shows

One text, added to Fast's conversation:

- **What woke it:** `Wake-up 14. The user just said m41.`, `New updates: u12.`,
  or `Something could not be done.`
- **The world's new entries,** each as [its line](../world/README.md#one-numbered-log).
- **The page:** its address, what is marked on it, whether the side panel is
  open, and, while the canvas is open, that the page is the scratch pad on it.
- **The page's text,** its first 14,000 characters, only when Fast has not
  been shown that text before.
- **The scratch pads there are,** and **the agents:** each with its state
  (busy, waiting for your answer, or free) and its last three tasks.

## From you to a slow agent

- A fact Fast can see in the world ("where am I?", "what is running?"): Fast
  says it.
- A judgment ("what do you think of this page?", "is this right?"): an agent
  gets it. Fast gives no opinion of its own.
- Anything else, a question that needs reading or a change: Fast routes it to
  an agent and says one short line.
- A follow-up to something recent: Fast sends it to the agent that did that
  work.
- Something about the page ("open", "scroll", "show me"): Fast does it itself,
  and only when asked.
- You ask again for work that is running: Fast says it is still being done.
- Half a sentence, or thinking aloud with nothing to do: nothing.

How it writes the task: your exact words as a value (`${m41}`), what your
words leave out, and nothing you did not ask for. The page and the marked
part are added by the program.

## Picking a slow agent

How Fast picks a slow agent for a piece of work. This protocol is text in
Fast's brief, not code.

```mermaid
flowchart TD
  work["A piece of work"] --> q1{"Does an agent already<br/>know this?"}
  q1 -->|"no, or not sure"| fresh["A fork of the base agent"]
  q1 -->|"yes"| q2{"Is that agent busy?"}
  q2 -->|"free, or waiting for an answer"| send["That agent"]
  q2 -->|"busy, and this changes<br/>the running work"| send
  q2 -->|"busy, and this is<br/>work of its own"| fork["A fork of that agent"]
```

### The three choices

- **The same agent: `a2.send`.** For a follow-up, or related work on what it
  just did. It costs nothing, but the work waits if the agent is busy.
- **A fork of an agent: `a2.fork`.** When that agent knows the area but is
  busy with something else. It costs a new conversation that starts with the
  other's context.
- **A fork of the base agent: `spawn`.** When nothing in the window fits, or
  Fast is not sure. It costs a new conversation with only the project
  knowledge.

**When unsure, a new agent.** A wrong new agent costs some context. A wrong
guess at an old one costs you a confused answer.

Which harness and model a new agent runs on is one more word in the same
`spawn`: see [Routing](../routing/README.md).

### What Fast routes on

- **When you said it.** Right after a task's update, it is likely a follow-up.
- **Where you said it.** The page and the marked part, against what each
  agent's tasks were about.
- **What you said.** Against the titles and updates of the recent tasks.
- **Whether the agent is busy.**

All of it comes from the tasks in the [world](../world/README.md). No agent
describes its own knowledge.

### How it is improved

- The rules are text in Fast's brief. Changing them is not a change to the
  loop.
- Every routing decision is logged: what you said, which agents existed,
  which was picked.
- A wrong pick becomes a case of [the evals](../../evals/README.md), and the
  brief changes until the case passes.

## From a slow agent to you

There is no fixed rule in the loop. Every update reaches Fast, and Fast
decides what you hear. An update is written to be spoken, so Fast passes it
on with `say(u12)` and does not retype it.

- **Finished, a question, or failed:** you hear it as it is, with its doubts.
- **Several at once:** one line for all of them.
- **Progress:** nothing.
- **A problem:** one line.
- **An update about the scratch pad that is open:** when it is longer than two
  sentences, one or two sentences of Fast's own, on what is on the pad now and
  what you have to decide. A pad is never read aloud.

This is guidance in Fast's brief. The brief is tuned, not the code.

## How Fast talks

One or two plain sentences, as "I". It never mentions agents, tasks or
models. When a script fails twice while you wait, the program says for it:
"I couldn't do that, something went wrong on my side."

## Not interrupting you

Voice is told never to talk over you: a line it is given while you speak, it
says when you stop.

## The loop

Fast needs a loop of its own. No existing agent harness wakes a model on a
voice hand-over and on task updates. It does six things:

1. Wakes Fast, on a hand-over and on updates.
2. Builds the input: the world changes since the last run, appended to Fast's
   conversation.
3. Runs `exec` in QuickJS, with [what a script can use](../world/README.md#what-a-script-can-use).
4. Carries out the script's calls: to Voice, to the page, to the slow agents.
5. Logs each run: what woke it, what Fast saw, the script, what it did.
6. Swaps Fast before its context fills: see [the swap](#swapping-fast-before-its-context-fills).

When things go wrong:

- **A script that throws** is given back to Fast with its error, for one more
  try. After two, nothing is done and the log has `run_failed`.
- **A call that cannot be carried out** becomes a `problem` in the world,
  which wakes Fast. The script's other calls are still carried out.

## Swapping Fast before its context fills

Fast must never make you wait for a compaction. Before its context fills, a
successor is built beside it and swapped in at a quiet moment.

```mermaid
flowchart LR
  limit["Soft limit<br/>reached"] --> build["Build the<br/>successor's context"]
  build --> boot["Boot it once<br/>prefix is cached"]
  boot --> wait["Wait for a<br/>quiet moment"]
  wait --> swap["Swap<br/>add what changed since"]
  swap --> drop["Drop the old one"]
```

The old Fast keeps serving until the swap. Nobody waits at any step.

### What the successor starts with

- the brief and the routing rules, complete, the rules read again from their
  file
- a summary of the earlier conversation
- the recent messages, as they were
- the page as it is now
- the recent tasks and their agents

Older tasks and agents are forgotten. That is accepted: when Fast cannot
place what you say, it starts a new agent.

### The steps

1. **Soft limit.** After a run, Fast's context has passed the limit.
2. **Summary.** A separate model call summarizes the earlier conversation in
   at most 300 words: what you are working on, what was decided, what you like
   and dislike, what is open.
3. **Boot.** The successor's context is sent to the model once, so the
   provider caches it.
4. **Quiet moment.** No wake-up is running, nothing waits for one, and you
   have not spoken for 1.5 seconds. It is looked for again every second.
5. **Swap.** The next wake-up goes to the successor, with the world changes
   since step 3, shown as already handled.
6. **Drop.** The old Fast is dropped. Idle agents that no task in the
   successor's window refers to are closed.

Step 5 is exact because the world is [one numbered log](../world/README.md#one-numbered-log):
"since step 3" is "after entry N".

### What can go wrong

- **No quiet moment before the hard limit.** Swap between two wake-ups
  anyway.
- **The cached prefix expired before the swap.** Boot again.
- **The summary could not be made.** Swap without it: the recent messages and
  the world are enough to go on.

### The numbers

In `live/fast.ts`:

- the soft limit: 60,000 tokens
- the hard limit: 150,000 tokens
- recent messages kept as they were: 30
- recent tasks kept: 12, each with its newest two updates
- how long the provider keeps the cached prefix: taken as 4 minutes, not
  checked

## What Fast is made of

- **The model:** `gpt-6-luna` at reasoning effort `medium`, on the priority
  service tier, which answers in about 1.4 seconds where the default tier
  takes 2 to 2.6. The [`fast` setting](../../README.md#settings) names another
  model. When the API refuses the model, the session goes on with
  `gpt-6.1-sol`, and the log gets a `fast` entry of kind `model_fallback`.
- **The sandbox:** QuickJS (`quickjs-wasi`), a new one for each script,
  stopped after 300 ms. In `live/sandbox.ts`.
- **The model loop:** the program's own, in `live/fast.ts`. It keeps the
  conversation itself and calls the Responses API with one tool, `exec`, which
  the model must call, one call at a time, with nothing stored at the
  provider and a 45-second limit on a call.
