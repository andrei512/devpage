---
page-type: concept
---
# The page part

The part of [the world](../README.md) named `page`: the page you are on, its
text, the part you marked and the side panel. Fast reads it and moves it.

## State

Where you are, what is marked, and who marked it: you, or a script's
`page.select`. The current mark survives a restart.

```js
{ address: "/docs/waiting-list/README.md", subject: { … }, by: "user" | "assistant" | null }
```

## Entry kinds

- **`page`**: what happened on the page, by you or by Fast, as one sentence:
  `The user marked "An offer stays open".` It does not wake Fast.

"What was marked when this was said" is not read from this part's state: a
[message](../session/README.md#entry-kinds) stores the mark at its own time.

## Functions

All are for Fast.

```js
page.go(address)
page.scroll(where) // "up", "down", "top", "bottom"
page.panel(open)
page.point(quote)  // the smallest block of the page that contains the quote
page.select(quote)
```

- **`page.go` takes only an address from the list of pages** Fast was shown.
  Going to a page closes the canvas.
- **A quote must be on the page exactly once.** No match, or more than one,
  is an error, found while the script can still fail. After a `page.go`, a
  `canvas.show` or a `canvas.close` in the same script the text is not known
  yet, and the quote is looked for when the call is carried out.
- **Each call adds a `page` entry** saying what Fast did.

These checks run inside the script, before any call is carried out: see
[how a script runs](../scripts/README.md#how-a-script-runs).

## Widgets

None of its own. The mark is drawn on the page itself, yours in the colour of
your voice and the assistant's in its own.
