# How themes reach code and diagrams

How highlighted code and Mermaid diagrams take their colors from the theme
tokens: the classes the highlighter writes and the variables pele draws with.
It is a mechanism page under [Themes](../README.md), for someone about to
change `assets/style.css`.

## Highlighted code

The highlighter marks code with classes named after syntax scopes, prefixed
`hl-` (`hl-keyword`, `hl-string`, `hl-comment`…), and `assets/style.css` maps
groups of those classes to the `--syntax-*` [tokens](../README.md#the-tokens):
comments (in italics), strings, numeric and language constants, keywords and
storage (in semibold), type names, function names, and tags, attribute names
and macros. Code the stylesheet doesn't map keeps the body text color. How the
server highlights code is in
[Serving pages](../../serving-pages/README.md#rendering-a-markdown-file).

## Diagrams

pele draws its SVG with its own CSS variables, and the stylesheet sets them on
every diagram from the theme tokens. The server draws the diagram
([Diagrams](../../diagrams/README.md#writing-a-diagram)), but the variables stay
variables in the SVG it puts in the page, so the browser resolves them against
the theme shown:

| pele variable | Theme token |
| --- | --- |
| `--pele-bg` | `--background` |
| `--pele-fg` | `--text` |
| `--pele-muted`, `--pele-line` | `--muted` |
| `--pele-surface` | `--surface` |
| `--pele-surface-alt`, `--pele-border` | `--border` |
| `--pele-accent` | `--accent` |
| `--pele-font`, `--pele-font-mono` | `--font`, `--font-mono` |

Because the colors are variables rather than values baked into the SVG, a
diagram changes with the page when the theme switches between light and dark.
The stylesheet also fixes `--pele-radius` at `8px`. The variables it doesn't
set keep pele's own defaults: with the 1.10.0 bundle, `--pele-error`,
`--pele-critical`, the `--pele-series-*` colors and the `--pele-zoom-*`
settings.

The stylesheet also styles the links in
[clickable diagrams](../../diagrams/README.md#clickable-diagrams): `.diagram a`
gets a pointer cursor and no underline, and on hover the shapes inside the link
(`rect`, `polygon`, `circle`, `ellipse`, `path`) take `--accent` as their
stroke, 2.5px wide.
