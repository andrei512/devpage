---
page-type: reference
---
# The devpage part

The part of [the world model](../README.md) named `devpage` makes your
project's pages something the agents can see and act on: the page you are on,
the part you marked, the list of pages, and comments. This page lists
everything the part declares. It is also the fullest example of
[a world model part](../../extend/README.md#a-world-model-part): it declares
all seven things a part can.

Turned off, the session still runs: you talk, work is given and reported. No
agent then knows about a page, and nothing in any prompt mentions one.

## State

Where you are, what is marked, and who marked it: you, or a script's
`devpage.select`. The current mark survives a restart.

```js
{ address: "/docs/waiting-list/README.md", subject: { … }, by: "user" | "assistant" | null, panel: false }
```

## Entry kinds

- **`page`:** what happened on the page, by you or by Fast, as one sentence:
  `The user marked "An offer stays open".` It does not wake Fast.

The part also adds two fields to every
[message](../session/README.md#entry-kinds) as it is stored: the page it was
said on, and the part marked then.

## Functions for Fast

```js
devpage.go(address)
devpage.scroll(where) // "up", "down", "top", "bottom"
devpage.panel(open)
devpage.point(quote)  // the smallest block of the page that contains the quote
devpage.select(quote)
```

- **`devpage.go` takes only an address from the list of pages** Fast was
  shown.
- **A quote must be on the page exactly once.** No match, or more than one,
  is an error, found while the script can still fail. After a `devpage.go` in
  the same script the text is not known yet, and the quote is looked for when
  the call is carried out.
- **Each call adds a `page` entry** saying what Fast did.

These checks run inside the script, before any call is carried out: see
[how a script runs](../scripts/README.md#how-a-script-runs).

## Functions for slow agents

```js
devpage_write_comment   // saves a comment on the part marked when the task was given
```

It is used only when you ask for a comment, and writes a devpage
[comment](../../../share/comments/README.md).

## Widgets

None of its own. The mark is drawn on the page itself, yours in the colour of
your voice and the assistant's in its own.

## What it adds to a task

Two lines, when there is something to say:

```
The page the user is on: /docs/waiting-list/README.md
Marked on it: "An offer stays open for three days"
```

They are the page and the mark stored with the message the task was made
from, not the ones at the time the agent starts.

## What it shows Fast

At each wake-up:

- **The page:** its address, what is marked on it, and whether the side panel
  is open.
- **The page's text,** its first 14,000 characters, only when Fast has not
  been shown that text before.
- **The list of pages:** every page of the project, with its address and
  title. `devpage.go` takes an address from it.

## What it tells each agent

- **Fast** (`fast.md`): that the user reads pages and marks parts of them;
  that "this" and "here" mean the marked part; when to go to a page and point
  instead of answering in words; that a fact on the page Fast can say itself.
- **Slow agents** (`slow.md`): that the project is drawn as pages by devpage;
  that a task names the page and the marked part; that a changed file shows
  on the page at once; when to write a comment.
- **Voice:** nothing.

How this text reaches a prompt is on
[The prompts](../../core/prompts/README.md#what-a-part-adds).
