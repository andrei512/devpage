---
page-type: task
---
# Viewing a design

How to look at a design: on its canvas, embedded in a page, and as a PNG, and
what its problems look like. It sits under [Designs](../README.md).

## Viewing a design

A design is a [document](../../writing-a-document/README.md#four-kinds): its
address shows it on a canvas, centred, at its own size — scaled down to fit
the window when it is bigger, never enlarged — and a reader can zoom with the
pinch gesture or **Ctrl**/**⌘** and the wheel, and pan by dragging or
scrolling. The canvas page has the light/dark switch in its corner, the same
switch every page has, and remembers the choice with them; the design is
drawn in the theme the page shows. `?theme=dark` or `?theme=light` on the
address shows that theme for that view without changing the remembered
choice — what an embed's **Open** link carries.

A design's **drawing** is its root's box plus anything painted outside it — a
shadow, a child placed past an edge — as in Figma's export. Every surface shows
the whole drawing, so nothing visible on one is missing from another; a design
whose paint stays inside its root draws at exactly its root's size.

A page **embeds** one with the image syntax — `![Checkout](../checkout.design)` —
or, in raw HTML, with an image of it — `<img src="checkout.design">`, written
however HTML allows. An embedded design is shown the same way: at its own
size, never enlarged, scaled down only as far as it needs to fit the page's
width and the window's height — on a slide, the room the slide leaves it —
centred on a quiet **backdrop** that sets it apart from the text. It is clickable like the design's own address, and a link in the
backdrop's corner opens it on its own canvas page, in the theme the embed
shows. Embedded designs follow the page's live updates and its light/dark
switch: on a dark page a design is drawn in its dark
[theme](../writing-a-design/README.md#themes) and its backdrop is dark, and a switch redraws it in the
browser, with nothing asked of the server. A design that uses its themes
also has a switch of its own on the backdrop, beside the link — the page's
sun and moon — which turns only that embed, design and backdrop, to the other
theme. It isn't remembered: the embed keeps the theme the reader turned it
to, even when the page switches, until the page loads again. In a PDF there is no window, so an embedded design prints
at its own size, scaled down only to fit the paper's width — or, on a slide,
the room the slide leaves it, as on screen. A PDF, a slide's PNG and a video
draw an embed in the theme of the page it is on, as the screen does.

While a design is open, saving any file it is made of — a `themes.yml` it
reads among them, or a font added to a font folder it uses — redraws it in place —
only the parts that changed are sent to the browser. A design deleted while
open shows its `no such file` problem, and comes back when it is written
again — after a branch switch, say — without a reload.

## Looking at a design as a PNG

`GET <path>.design.png` answers with the design's whole drawing as a PNG,
at 1×, from the same layout and the same scene as the canvas
([one renderer](../../constraints/a-design-has-one-renderer.md)):

```sh
curl -s http://localhost:3030/docs/designs/checkout/checkout.design.png -o checkout.png
```

`?theme=dark` paints it in its dark [theme](../writing-a-design/README.md#themes) —
`checkout.design.png?theme=dark` — and `?theme=light`, or no `theme`, in its
light one.

With no server running, `devpage png` draws the same picture to a file:

```sh
devpage png docs/designs/checkout/checkout.design               # checkout.png
devpage png docs/designs/checkout/checkout.design --theme dark -o dark.png
```

A design with problems isn't drawn: `devpage png` prints them as
`devpage check` does and exits `1`.

This is how an agent looks at its work: write the file, fetch the PNG, look,
fix — in both themes when the design uses them. devpage keeps every design it has read resolved in memory and paints the
PNG itself on the CPU, without a browser, so the answer arrives as fast as a
page. A design of any size has a PNG: it is streamed band by band, and after
an edit only the part the edit touched is painted again. A page can link to or show the PNG too —
`![Checkout](../checkout.design.png)` — and `devpage check` treats that link as
the design itself.

| Answer | When |
| --- | --- |
| `200`, `image/png` | the design is valid: its whole drawing at 1× |
| `400`, `theme: expected light or dark, found "<value>"` | `theme` is anything but `light` or `dark` |
| `422`, the problems as `text/plain`, one per line | the design has problems |
| `404`, `nothing at <address>` | `<path>.design` isn't a file in the served folder, as for any other path |

A real file named `checkout.design.png` beside the design wins: it is served
as it is, and a link to it is checked as that file.

## Problems

A design's problems are shown on the canvas page and on any page that embeds
it, reported by `devpage check` like a broken link
([Checking links](../../checking-links/README.md)), and returned instead of a PNG:

```text
docs/designs/checkout/checkout.design:7:11: Button has no prop "lable"
docs/designs/checkout/button.design:2:21: padding: expected pixels or an array of 2 or 4, found "12px"
docs/designs/checkout/cart.design:1:1: import ./row.design — no such file
docs/designs/checkout/cart.design:4:30: theme.srface: the themes have no srface; they have accent, background, border, text
docs/designs/checkout/cart.design:9:9: font: no font named figtre; the themes name figtree, lora
docs/designs/checkout/cart.design:12:24: weight: Lora has no 200; it has 400, 500, 600 and 700
```

The [themes](../writing-a-design/README.md#themes) have problems of their own, each at the reference or
at the line of the `themes.yml` that is wrong:

- `theme.x` in a design with no `themes.yml` above it;
- a name the themes don't have, listing those they do;
- a name only one theme has, once the files are merged;
- a `themes.yml` with `kind: themes` that is malformed — a value that isn't a
  color, `light` or `dark` missing, or either one not a map of names to
  colors;
- `theme` as a prop's or an import's name, and `{theme.x}` in a `Text`'s
  text, where a color can't go.

[Fonts](../writing-a-design/README.md#fonts) have problems of their own:

- at a `Text`'s `font`: a name the fonts don't have, listing those they do,
  or a value that isn't a name;
- at the `Text`'s `weight`, or at the `Text` when it leaves its weight at
  400: a weight the font doesn't have, listing those it has;
- at a `fonts` entry of the `themes.yml`, once however many texts use it: a
  name that isn't lowercase letters, digits and `-`, or a path that isn't
  text; and when a text uses the font, no such file or folder, a file that
  isn't a `.ttf` or `.otf` font — a WOFF, a WOFF2 or a collection of several
  fonts among them — a file larger than 4 MB, or a folder with no `.ttf` or
  `.otf` file, with only italic faces, with faces of more than one family,
  naming each, or with two files of one weight.

While a design has problems the canvas keeps showing the last valid drawing,
under the list of problems.

The rule that the canvas and the PNG are drawn by the same code is
[A design has one renderer](../../constraints/a-design-has-one-renderer.md).
