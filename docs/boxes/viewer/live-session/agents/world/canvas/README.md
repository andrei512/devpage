---
page-type: concept
---
# The canvas part

The part of [the world](../README.md) named `canvas`: the
[scratch pad](../../../scratch-pad/README.md) open over the page. While it is
open, the pad is the page Fast reads.

## State

The open pad, with a number that changes whenever its file does, or `null`
while the canvas is closed. The open pad survives a restart.

```js
{ canvas: { pad: "pads/reminding-about-an-offer.md", version: 1763500000000 } }
```

## Entry kinds

None of its own. Opening and closing the canvas, by you or by Fast, is a
[`page` entry](../page/README.md#entry-kinds): the pad becomes the page you
are on, and a mark left on the page under it is dropped.

## Functions

All are for Fast.

```js
canvas.pad(title)    // the address of the scratch pad for an idea
canvas.show(address) // opens the canvas on that pad, over the page
canvas.close()
```

- **`canvas.pad` only names a pad.** It answers the address made from the
  idea's title, inside the script.
- **`canvas.show` takes only a pad made in this script, or one from the list
  of pads** Fast was shown. A pad that is not there yet is made with its
  title, so there is something to show at once.

## Widgets

`DevpageLiveCanvas`, in the `float` place: the canvas the pad is drawn on
([the live session's widgets](../../../../on-the-page/widgets/system-widgets/README.md#the-live-sessions-widgets)).
