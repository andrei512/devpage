# Designs

A **design** is a UI drawn as code: a `.design` file an agent writes, which
devpage renders on a canvas at high fidelity and lets a reader click through
as a **prototype**. Designs exist so that an agent can show a screen or a flow
before anyone builds it, and so that people can agree on it fast — with the
agent, or with each other on a [deployed](../deploying/README.md) devpage. It is
a concept and task article under the [viewer](../README.md); how a design is
parsed, laid out, drawn and streamed is in the
[design engine](../design-engine/README.md).

```mermaid
flowchart LR
  agent["Agent"] -->|"writes"| file["checkout.design"]
  file -->|"imports"| button["button.design"]
  file --> engine["Design engine<br/>parse · resolve · lay out · paint"]
  button --> engine
  engine -->|"changed nodes, over gRPC-Web"| canvas["Canvas in the browser<br/>click through the prototype"]
  engine -->|"GET checkout.design.png"| png["PNG<br/>the agent looks at its work"]
  click engine href "../design-engine/README.md" "The design engine"
```

Humans never edit a design. They look at it, comment on the page that embeds
it, and the agent changes the file. A design is not documentation of the
current UI and nobody keeps it in sync with the code; it is a prototype, and
the code is the design of what is built. What a design must be is **valid**:
every design it uses exists, every value parses, and devpage reports anything
else as a [problem](#problems).

## A first design

The example's button, `docs/designs/checkout/button.design`, whole:

```jsx
// A button: one line of text on a filled, rounded box.
<Frame props={{ label: "Button", fill: "#2563eb" }}
       layout="row" padding={[12, 20]} radius={8} fill={fill}
       justify="center" shadow={{ y: 2, blur: 6, color: "#1e3a8a33" }}>
  <Text size={16} weight={600} color="#ffffff">{label}</Text>
</Frame>
```

A screen using it, cut down from the example's
`docs/designs/checkout/checkout.design` to its bones:

```jsx
import Button from "./button.design"

<Frame name="Checkout" width={390} height={844} fill="#f8fafc"
       layout="column" gap={16} padding={24}>
  <Text size={28} weight={700}>Your cart</Text>
  <Button label="Pay $42" onClick="navigate ./confirmation.design" />
</Frame>
```

Here is the whole checkout screen, embedded in this page:

![Checkout](../../../designs/checkout/checkout.design)

Open either file in devpage — `devpage docs/designs/checkout/checkout.design`, or
link to it from any page — and it is drawn on a canvas. Click **Pay $42** and
the prototype moves to `confirmation.design`.

## One design per file

Every design is one `.design` file, and a design is made of other designs.
There is no separate kind of "component": a button is a design that other
designs use. A file:

- starts with any number of **imports**, one per line —
  `import Button from "./button.design"` — each a name and a path relative to
  the file. The path must name a `.design` file that exists in the
  [served folder](../constraints/the-served-folder-is-the-boundary.md),
  whether the design uses the import or not. Each import takes a name of its
  own, and none takes a built-in element's name — `Frame`, `Text`, `Rect`,
  `Ellipse`, `Vector` or `Image`.
- holds exactly one **root element**, which is a `Frame`. Its `width` and
  `height` are the design's size; a root sized `"hug"` takes its content's
  size. A root takes no `x` or `y`: it has no place of its own, and an
  instance goes where the instance's own `x` and `y` say.

Elements, arrays and objects nest at most 256 deep, counting through the
designs a design uses — far deeper than any real design. A number or a string
inside them is not a level of its own.

Using an imported design places an **instance** of it: `<Button label="Pay" />`
draws `button.design` with `label` set to `"Pay"`. An instance takes the
attributes [every element takes](elements-and-attributes/README.md#attributes) — `name`, `x`, `y`, `width`,
`height`, `opacity` and `onClick`, each applied to the used design's root —
and the design's props; nothing else. Its look — fill, stroke, corners — is
the design's own, changed only through its props. A design that uses itself, directly or through
others, is a problem.

`//` starts a comment that runs to the end of the line, outside elements and
between attributes.

## Props

A design declares the values it takes in its root's `props` attribute, each
with a default: `props={{ label: "Button", fill: "#2563eb" }}`. Only the root
declares props; `props` on any other element is a problem. Inside the
design, `{label}` puts a prop's value where it stands — as a whole attribute
value (`fill={fill}`) or inside a text (`<Text>Hi, {name}</Text>`). An instance
that leaves a prop out gets its default. Naming a prop the design doesn't
declare is a problem. There are no expressions: a value is a literal or one
prop.

## Elements and attributes

Six elements draw — `Frame`, `Text`, `Rect`, `Ellipse`, `Vector` and `Image` —
and an imported name places an instance. Their node types and attributes
follow Figma's document model, written as JSX attributes rather than Figma's
JSON; every one, with its values and defaults, is on
[Elements and attributes](elements-and-attributes/README.md).

## Prototypes

A prototype is designs linked by **interactions**. `onClick` on any element
takes one:

| Interaction | What a click does |
| --- | --- |
| `"navigate <path>"` | shows the design at `path` in place of this one |
| `"back"` | returns to the design shown before |
| `"overlay <path>"` | shows the design at `path` centered over this one, on a dimmed backdrop |
| `"close"` | closes the overlay this element is in |

A path is relative to the file the `onClick` is written in, like every path in
a design, so an interaction inside an imported design works wherever that
design is used. A path must name a `.design` file that exists in the served
folder; anything else is a problem.

The element under the pointer that has an interaction takes the click, and a
pointer over one turns into a hand. A navigation changes the address, so the
browser's back button walks back through the prototype, and every step can be
linked to. Each state of a prototype is a design of its own.

## Viewing a design

A design is a [document](../writing-a-document/README.md#four-kinds): its
address shows it on a canvas, centred, at its own size — scaled down to fit
the window when it is bigger, never enlarged — and a reader can zoom with the
pinch gesture or **Ctrl**/**⌘** and the wheel, and pan by dragging or
scrolling. The canvas page has the light/dark switch in its corner, the same
switch every page has, and remembers the choice with them.

A design's **drawing** is its root's box plus anything painted outside it — a
shadow, a child placed past an edge — as in Figma's export. Every surface shows
the whole drawing, so nothing visible on one is missing from another; a design
whose paint stays inside its root draws at exactly its root's size.

A page **embeds** one with the image syntax — `![Checkout](checkout.design)` —
or, in raw HTML, with an image of it — `<img src="checkout.design">`, written
however HTML allows. An embedded design is shown the same way: at its own
size, never enlarged, scaled down only as far as it needs to fit the page's
width and the window's height — on a slide, the room the slide leaves it —
centred on a quiet stage that sets it apart from the text. It is clickable like the design's own address, and a link in the
stage's corner opens it on its own canvas page. Embedded designs follow the
page's live updates and its light/dark switch: a design's own colours never
change, but the page behind any part it leaves transparent does. In a PDF there is no window, so an embedded design prints
at its own size, scaled down only to fit the paper's width — or, on a slide,
the room the slide leaves it, as on screen.

While a design is open, saving any file it is made of redraws it in place —
only the parts that changed are sent to the browser. A design deleted while
open shows its `no such file` problem, and comes back when it is written
again — after a branch switch, say — without a reload.

## Looking at a design as a PNG

`GET <path>.design.png` answers with the design's whole drawing as a PNG,
at 1×, from the same layout and the same scene as the canvas
([one renderer](../constraints/a-design-has-one-renderer.md)):

```sh
curl -s http://localhost:3030/docs/designs/checkout/checkout.design.png -o checkout.png
```

This is how an agent looks at its work: write the file, fetch the PNG, look,
fix. A page can link to or show the PNG too — `![Checkout](checkout.design.png)`
— and `devpage check` treats that link as the design itself. A real file named
`checkout.design.png` beside the design wins: it is served as it is. devpage keeps every design it has read resolved in memory and paints the PNG
itself on the CPU, without a browser, so the answer arrives as fast as a
page. A design
with problems answers `422` with the problems as plain text, one per line, as
does a drawing bigger than 16,384 pixels a side, which is too big to paint; an
address that isn't a design file in the served folder answers `404`, as any
other path does.

## Problems

A design's problems are shown on the canvas page and on any page that embeds
it, reported by `devpage check` like a broken link
([Checking links](../checking-links/README.md)), and returned instead of a PNG:

```text
docs/designs/checkout/checkout.design:7:11: Button has no prop "lable"
docs/designs/checkout/button.design:2:21: padding: expected pixels or an array of 2 or 4, found "12px"
docs/designs/checkout/cart.design:1:1: import ./row.design — no such file
```

While a design has problems the canvas keeps showing the last valid drawing,
under the list of problems.

The rule that the canvas and the PNG are drawn by the same code is
[A design has one renderer](../constraints/a-design-has-one-renderer.md).

## Topics

- [Elements and attributes](elements-and-attributes/README.md) — every
  element, attribute, value and default a design can use.
