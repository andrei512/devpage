---
page-type: task
---
# Writing a design

How to write a `.design` file: a first design, how designs use each other,
props, themes, fonts, the elements, and the interactions that make a
prototype. It sits under [Designs](../README.md), which says what a design
is.

## A first design

The example's button, `docs/designs/checkout/button.design`, whole:

```jsx
// A button: one line of text on a filled, rounded box.
<Frame props={{ label: "Button", fill: theme.accent }}
       layout="row" padding={[12, 20]} radius={8} fill={fill}
       justify="center" shadow={{ y: 2, blur: 6, color: theme.accentShadow }}>
  <Text font="figtree" size={16} weight={600} color="#ffffff">{label}</Text>
</Frame>
```

A screen using it, cut down from the example's
`docs/designs/checkout/checkout.design` to its bones:

```jsx
import Button from "./button.design"

<Frame name="Checkout" width={390} height={844} fill={theme.background}
       layout="column" gap={16} padding={24}>
  <Text font="lora" size={28} weight={700} color={theme.text}>Your cart</Text>
  <Button label="Pay $42" onClick="navigate ./confirmation.design" />
</Frame>
```

Here is the whole checkout screen, embedded in this page:

![Checkout](../../../../designs/checkout/checkout.design)

Open either file in devpage — `devpage docs/designs/checkout/checkout.design`, or
link to it from any page — and it is drawn on a canvas. Click **Pay $42** and
the prototype moves to `confirmation.design`.

Every `theme.` name comes from `docs/designs/checkout/themes.yml`, the
[themes](#themes) beside them, so the same files draw a light checkout on a
light page and a dark one on a dark page. The same file names the checkout's
two [fonts](#fonts), `lora` for its headings and `figtree` for the rest, from
`docs/designs/checkout/fonts/`.

## One design per file

Every design is one `.design` file, and a design is made of other designs.
There is no separate kind of "component": a button is a design that other
designs use. A file:

- starts with any number of **imports**, one per line —
  `import Button from "./button.design"` — each a name and a path relative to
  the file. The path must name a `.design` file that exists in the
  [served folder](../../constraints/the-served-folder-is-the-boundary.md),
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
attributes [every element takes](../elements-and-attributes/README.md#attributes) — `name`, `x`, `y`, `width`,
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

A value given as a prop is read where it was written, wherever the prop ends
up: a path — an `Image`'s `src`, an interaction's target — is relative to the
file that wrote it, and a `Text`'s `font` is looked up in that file's
[fonts](#fonts). `<Avatar src="./me.png" />` shows the `me.png` beside the
file that wrote that line, not beside `avatar.design`. A default in `props`
is written by the design's own file.

## Themes

A design's colors can follow the light/dark switch. A file named `themes.yml`
holds the design's own two [themes](../../themes/README.md) — light and dark —
each a set of named colors:

```yaml
kind: themes
light:
  background: "#f8fafc"
  text: "#0f172a"
dark:
  background: "#0f172a"
  text: "#f1f5f9"
```

`theme.<name>` is a color wherever a color goes, and stands for that name's
value in the theme the design is drawn in:

```jsx
<Frame fill={theme.background} stroke={theme.border}
       shadow={{ y: 8, blur: 24, color: theme.shadow }}
       props={{ accent: theme.accent }}>
  <Text color={theme.text}>Hi</Text>
  <Frame fill="linear-gradient(90deg, theme.accent 0%, #7c3aed 100%)" />
</Frame>
```

— a fill, a stroke, a text's color, a shadow's color, a gradient's stop
inside its string, a prop's default and an instance's attribute
(`<Button fill={theme.accent} />`). A color written as `#rrggbb` stays the
same in both themes, and so does a default — a text's `#0f172a` — so a
design that follows the themes names every color it wants to change.

- **Which `themes.yml`.** A design uses the nearest `themes.yml` up the
  folder tree from its own file, as far as the
  [served folder](../../constraints/the-served-folder-is-the-boundary.md)'s top;
  nothing imports it. A `themes.yml` lower down overrides only the names it
  lists, in each theme, and takes the rest from the ones above it, so one
  file at the top can hold a product's palette and a folder can change a few
  of its colors. A `themes.yml` whose `kind` isn't `themes` is another tool's
  file, and devpage passes over it.
- **What a theme holds.** Colors only — `#rgb`, `#rgba`, `#rrggbb` or
  `#rrggbbaa`, in quotes, since YAML reads an unquoted `#` as a comment. A
  name is letters, digits, `_` and `-`. Every `themes.yml` has both `light`
  and `dark`, and after the files are merged every name a design uses has a
  value in both, so dark never lacks a color light has.
- **Where a name is looked up.** In the themes of the file `theme.` is
  written in, as a relative path is. A `theme.accent` passed as a prop to a
  design in another folder, or a prop default that names one, keeps the color
  of the file that wrote it.
- **`theme` is reserved.** No prop and no import may be called `theme`.

A design never picks its theme; where it is shown does — see
[Viewing a design](../viewing-a-design/README.md#viewing-a-design).

## Fonts

A `Text` with no `font` is set in **Inter**, which devpage carries inside it —
regular, medium, semibold and bold. Any other font is a **font file** in the
[served folder](../../constraints/the-served-folder-is-the-boundary.md), which a
`themes.yml` gives a short name; a `Text` names the font it is set in, and a
design can mix as many as it likes. devpage never uses a font installed on the
machine: the canvas is WebAssembly in a browser that can't read them, and the
PNG is drawn wherever devpage runs.

```yaml
kind: themes
fonts:
  figtree: ./fonts/Figtree       # a folder holding one family
  lora: ./fonts/Lora/Lora.ttf    # or one font file
light:
  text: "#0f172a"
dark:
  text: "#f1f5f9"
```

```jsx
<Text font="lora" size={28} weight={700}>Your cart</Text>
<Text font="figtree">Size M · Sand</Text>
```

- **Naming a font.** `fonts` maps a **font name** — lowercase letters,
  digits and `-` — to a path relative to that `themes.yml`: a font file, or a
  folder holding one family's files. Names cascade as colors do: a design
  sees the names of every `themes.yml` from its folder up, a lower file adding
  names and overriding the ones it repeats. Fonts are no part of a theme —
  light and dark set text in the same fonts — and `fonts` sets no default.
- **Using one.** `font` on a `Text` takes a name, never a path, and is looked
  up in the fonts of the file it is written in; given as a prop, in the fonts
  of the file that wrote the value. A design that shares a look takes the font
  as a prop: `<Heading font="lora" />`.
- **What a font file is.** A `.ttf` or `.otf` of one font, at most 4 MB. A
  folder's fonts are the `.ttf` and `.otf` files directly in it; anything else
  there, a licence say, is passed over, and so are its italic and oblique
  faces, since a text is always upright.
- **Weight picks the face.** `weight` means the same in every font. Inter has
  400, 500, 600 and 700. Any other font takes a weight from 100 to 900, in
  steps of 100, that it has: a variable font, any weight its weight axis
  reaches — drawn at that point of the axis; a folder of static files, the
  weight of one of its files.
- **Why 4 MB.** Every font file a design uses goes, whole, to every browser
  that shows the design, with its first update and before it draws. A whole
  Latin family is well under 1 MB — static or variable, Greek and Cyrillic
  included; each Inter face devpage carries is 0.4 MB. A font of 4 MB or more
  covers a script of thousands of characters, Chinese or Japanese, and should
  be subset to the characters the design shows.
- **WOFF and WOFF2.** The web's compressed fonts aren't read: neither backend
  can decompress them without carrying a decompressor into the browser's
  module. Convert one back to a `.ttf` — `fonttools ttLib.woff2 decompress`
  does it.

A font file comes with its licence: one checked into a repository brings the
licence along, as the checkout's `fonts/Figtree/OFL.txt` and
`fonts/Lora/OFL.txt` do.

## Elements and attributes

Six elements draw — `Frame`, `Text`, `Rect`, `Ellipse`, `Vector` and `Image` —
and an imported name places an instance. Their node types and attributes
follow Figma's document model, written as JSX attributes rather than Figma's
JSON; every one, with its values and defaults, is on
[Elements and attributes](../elements-and-attributes/README.md).

## Prototypes

A prototype is designs linked by **interactions**. `onClick` on any element
takes one:

| Interaction | What a click does |
| --- | --- |
| `"navigate <path>"` | shows the design at `path` in place of this one |
| `"back"` | returns to the design shown before |
| `"overlay <path>"` | shows the design at `path` centered over this one, which it dims |
| `"close"` | closes the overlay this element is in |

A path is relative to the file that wrote it, like every path in a design —
[passed as a prop](#props) too — so an interaction inside an imported design
works wherever that design is used. A path must name a `.design` file that exists in the served
folder; anything else is a problem.

The element under the pointer that has an interaction takes the click, and a
pointer over one turns into a hand. A navigation changes the address, so the
browser's back button walks back through the prototype, and every step can be
linked to. Each state of a prototype is a design of its own.
