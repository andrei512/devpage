# Elements and attributes

Every element a design can use and every attribute each one takes, with its
values and defaults. It is the reference page under
[Designs](../README.md), for an agent or a person writing a `.design` file;
how the file is put together — imports, one root, props and interactions — is
on that page.

## Elements

| Element | Draws | Children |
| --- | --- | --- |
| `Frame` | a box: fill, stroke, corners, shadow; lays out and clips its children | any elements |
| `Text` | a run of text in the bundled typeface | text and `{prop}` only |
| `Rect` | a rectangle | none |
| `Ellipse` | an ellipse filling its box | none |
| `Vector` | an SVG path, `d="M…"`, scaled from its `viewBox` into its box | none |
| `Image` | a PNG or JPEG file, `src` relative to the design, covering its box | none |
| an imported name | an instance of that design | none |

The node types and their attributes follow Figma's document model — frames
with auto layout, the same fills, strokes and effects — written as attributes
in JSX syntax rather than as Figma's JSON. They are the set the
[example designs](../../../../designs/checkout/checkout.design) use; another
arrives when a design needs it.

## Attributes

Attributes are written as in JSX. A value is a string, or a number, boolean,
array, object, prop or theme color in braces: `"row"`, `{16}`, `{true}`,
`{[12, 20]}`, `{{ y: 4, blur: 12, color: "#0f172a33" }}`, `{label}`,
`{theme.text}`.

- A string is in double or single quotes, and has no escapes: a backslash is
  just a backslash, as in JSX.
- An attribute written bare, with no value — `<Frame clip>` — is `{true}`.
- A name may hold letters, digits, `_` and `-`, not first: `aria-label`.
- A number goes from −1,000,000 to 1,000,000, written in braces or inside a
  string — a gradient's angle and stops, a `viewBox`, a path's `d`.
- An attribute may be given once on an element or an instance; given twice,
  it is a problem.

Every element takes:

| Attribute | Value | Default |
| --- | --- | --- |
| `name` | a label for the node, shown in problems | the element's name |
| `x`, `y` | position in pixels inside a parent with no layout; in a parent with `layout="row"` or `"column"`, which places its children itself, and on a design's root, which has no parent, they are a problem | `0` |
| `width`, `height` | pixels from `0` up, `"hug"` (fit the content) or `"fill"` (take the free space in the parent's layout direction, or stretch across it — only in a parent with `layout="row"` or `"column"`; a design's root shown on its own has no parent, and there `"fill"` hugs) | `"hug"` |
| `opacity` | `0` to `1` | `1` |
| `onClick` | an [interaction](../README.md#prototypes) | none |

Boxes — `Frame`, `Rect`, `Ellipse`, `Vector` and `Image` — also take:

| Attribute | Value | Default |
| --- | --- | --- |
| `fill` | a color, or `"linear-gradient(<angle>deg, <color> <stop>%, …)"` | none |
| `stroke` | a color | none |
| `strokeWidth` | pixels from `0` up, drawn inside the box | `1` |
| `radius` | pixels from `0` up, or `[topLeft, topRight, bottomRight, bottomLeft]` | `0` |
| `shadow` | `{ x, y, blur, spread, color }`, each optional: the box's shape, each corner's radius and the box itself widened by `spread`, moved by `x` and `y`, blurred by `blur` and drawn outside the box | none |

A color is `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`, the same in both
themes, or `theme.<name>` in braces — `fill={theme.surface}` — the value
`name` has in the theme the design is drawn in, from its
[themes](../README.md#themes). A gradient names a theme's color inside its
string, where a stop's color goes:
`"linear-gradient(90deg, theme.accent 0%, #7c3aed 100%)"`.

`Frame` lays out its children with Figma's auto layout, and can clip them:

| Attribute | Value | Default |
| --- | --- | --- |
| `clip` | `{true}` to cut the children to the frame's shape, corners included | `{false}` |
| `layout` | `"row"`, `"column"`, or `"none"` — children placed by their `x` and `y`, and a hugging frame as big as their furthest right and bottom edges | `"none"` |
| `gap` | pixels from `0` up between children | `0` |
| `padding` | pixels from `0` up, `[vertical, horizontal]` or `[top, right, bottom, left]` | `0` |
| `align` | across the layout direction: `"start"`, `"center"`, `"end"` | `"start"` |
| `justify` | along it: `"start"`, `"center"`, `"end"`, `"space-between"` | `"start"` |

`Text` takes:

| Attribute | Value | Default |
| --- | --- | --- |
| `size` | pixels from `0` up | `14` |
| `weight` | `400`, `500`, `600` or `700` | `400` |
| `color` | a color | `#0f172a` |
| `lineHeight` | a multiple of the size, from `0` up | `1.4` |
| `textAlign` | `"left"`, `"center"`, `"right"` | `"left"` |

A text wraps at its width when it has one and runs on one line when it hugs.
Text is set in Inter, which devpage carries inside it, so a design looks the
same on every machine and in every PNG.

`Vector` takes `d` (SVG path data) and `viewBox` (`"minX minY width
height"`, the path's own bounds when left out). The path is scaled into the
vector's box keeping its proportions, and its `fill` and `stroke` paint the
path rather than the box. `Image` takes `src`, a PNG or JPEG file that
decodes whole; a broken or truncated one is a problem at `src`. A file larger
than 4,096 pixels a side is drawn from its pixels resampled to fit that, and
the images one design shows must fit the GPU together — a problem at the
`src` of the first that doesn't; see
[the GPU's image atlas](../../design-engine/README.md#the-gpus-image-atlas).
Hugging, a vector takes its view box's size and an image its own size in
pixels; given only one of `width` and `height`, either keeps its proportions.

An attribute an element doesn't take, a value of the wrong type, or an unknown
element is a problem — never ignored — so an agent learns about a typo from
devpage rather than from a reader.
