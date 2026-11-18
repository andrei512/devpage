---
page-type: reference
---
# The page module

A widget imports `devpage` to read the page it is on and steer it: what is shown, what the reader selected or picked, where a quoted part is, and the address of an extension. It is how a widget in a [place](../places/README.md) works with the text beside it. This box belongs to [widgets](../README.md).

```tsx
import { useEffect, useState } from "react";
import { page } from "devpage";

export default function Selected() {
  const [part, setPart] = useState(page.selection());
  useEffect(() => page.on("select", setPart), []);
  return <p>{part ? part.quote : "Nothing selected"}</p>;
}
```

The import map of every page names `devpage`; the module is `core/assets/page.js`, and what it exports is made by the [page script](../../page-script/README.md).

## A part of a page

A part is `{ page, quote, prefix, suffix }`: a source's address, words in it, and up to 32 characters of the text before and after them. It is what a [comment](../../comments/README.md) is anchored by, so a part can be sent as a comment's anchor and a comment's anchor used as a part. `prefix` and `suffix` may be left out; with them, the right one of several equal quotes is found.

A source is one Markdown file shown on the page: the page itself, a doc's chapter, a deck's slide. Addresses are the document's own, as the server names them: `/docs/guide.md`.

## `page`

| Call | What it does |
| --- | --- |
| `page.current()` | `{ address, fragment, sources }`: the page shown, its `#fragment` and its sources' addresses. |
| `page.text(source?)` | A source's text as the reader sees it, white space collapsed; the first source's when none is given. `null` when the page has no such source. |
| `page.selection()` | The selected text as a part; `null` when nothing is selected or the selection isn't inside one source. |
| `page.range(part)` | The part's DOM `Range` on the page shown; `null` when it isn't there. |
| `page.go(address)` | Opens an address as a click on a link to it would: a page of the document in place, anything else whole. Resolves once it is shown. |
| `page.mark(name, parts)` | Marks the parts as `mark.devpage-mark[data-mark="<name>"]`, in place of what the name marked before; with no parts, takes the name's marks away. Marks follow the reader: each page shown is marked wherever it has them. |
| `page.pick(on)` | Turns picking on or off. |
| `page.panel(open)` | Opens or closes [the panel](../places/README.md#the-panel); `null` leaves it to the window's width. |
| `page.on(event, listener)` | Calls `listener` on each event; returns the function that stops it. |

### Events

| Event | When | The listener gets |
| --- | --- | --- |
| `change` | A page was shown: a link, a live update, Back. | What `page.current()` returns. |
| `select` | The selection rests, or is gone. | A part, or `null`. |
| `pick` | The reader clicked a block while picking, or beside the text. | The block's part, or `null`. |

### Picking

While picking is on, the block under the pointer — a paragraph, list item, heading, quote, code block, table or diagram — is outlined, and a click on its text is a pick: no comment starts. A click on a link, a button or another control does what it always does. A click beside the text picks nothing, and the listeners get `null`: a widget that kept the last pick lets go of it.

### The panel

`page.panel(true)` opens the panel on any window; `page.panel(false)` closes it and gives its room back to the page.

## `base`

`base(id)` is the address the document's [extension](../../extensions/README.md) `id` answers under, ending in `/`, so `fetch(base("toy") + "live/state")` asks [its process](../../extensions/reference/README.md#process). It throws when the document has no such extension.
