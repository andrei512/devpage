---
page-type: reference
---
# Theme tokens

Every token the light and dark themes are made of, with what each one
colors. It is the reference page under [Themes](../README.md).

## The tokens

The viewer has a light and a dark theme. Which one shows is the one the
reader switched to with the [theme switch](../README.md#choosing-a-theme), or — until they
switch, and on a page without one — the system setting
(`prefers-color-scheme`). The chosen theme is set on the page as
`data-theme="light"` or `data-theme="dark"` on `<html>`, before it paints, and
every dark value hangs off that. The page's colors come from seven tokens,
defined in `core/assets/style.css`:

| Token | Used for | Light | Dark |
| --- | --- | --- | --- |
| `--background` | page background | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#ffffff;vertical-align:-0.15em;margin-right:0.4em"></span>`#ffffff` | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#000000;vertical-align:-0.15em;margin-right:0.4em"></span>`#000000` |
| `--surface` | sidebar, quotes | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#f5f5f7;vertical-align:-0.15em;margin-right:0.4em"></span>`#f5f5f7` | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#1d1d1f;vertical-align:-0.15em;margin-right:0.4em"></span>`#1d1d1f` |
| `--text` | body text | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#1d1d1f;vertical-align:-0.15em;margin-right:0.4em"></span>`#1d1d1f` | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#f5f5f7;vertical-align:-0.15em;margin-right:0.4em"></span>`#f5f5f7` |
| `--muted` | secondary text, group headings | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#6e6e73;vertical-align:-0.15em;margin-right:0.4em"></span>`#6e6e73` | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#a1a1a6;vertical-align:-0.15em;margin-right:0.4em"></span>`#a1a1a6` |
| `--border` | rules, borders, hover | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#d2d2d7;vertical-align:-0.15em;margin-right:0.4em"></span>`#d2d2d7` | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#424245;vertical-align:-0.15em;margin-right:0.4em"></span>`#424245` |
| `--accent` | links, the current sidebar entry | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#0066cc;vertical-align:-0.15em;margin-right:0.4em"></span>`#0066cc` | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#2997ff;vertical-align:-0.15em;margin-right:0.4em"></span>`#2997ff` |
| `--code` | code background | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#f5f5f7;vertical-align:-0.15em;margin-right:0.4em"></span>`#f5f5f7` | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#1d1d1f;vertical-align:-0.15em;margin-right:0.4em"></span>`#1d1d1f` |

Seven more tokens color highlighted code. Their values are Xcode's default
light and dark palettes:

| Token | Light | Dark |
| --- | --- | --- |
| `--syntax-keyword` | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#9b2393;vertical-align:-0.15em;margin-right:0.4em"></span>`#9b2393` | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#ff7ab2;vertical-align:-0.15em;margin-right:0.4em"></span>`#ff7ab2` |
| `--syntax-string` | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#c41a16;vertical-align:-0.15em;margin-right:0.4em"></span>`#c41a16` | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#ff8170;vertical-align:-0.15em;margin-right:0.4em"></span>`#ff8170` |
| `--syntax-number` | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#1c00cf;vertical-align:-0.15em;margin-right:0.4em"></span>`#1c00cf` | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#d9c97c;vertical-align:-0.15em;margin-right:0.4em"></span>`#d9c97c` |
| `--syntax-comment` | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#5d6c79;vertical-align:-0.15em;margin-right:0.4em"></span>`#5d6c79` | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#7f8c98;vertical-align:-0.15em;margin-right:0.4em"></span>`#7f8c98` |
| `--syntax-type` | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#0b4f79;vertical-align:-0.15em;margin-right:0.4em"></span>`#0b4f79` | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#dabaff;vertical-align:-0.15em;margin-right:0.4em"></span>`#dabaff` |
| `--syntax-function` | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#326d74;vertical-align:-0.15em;margin-right:0.4em"></span>`#326d74` | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#67b7a4;vertical-align:-0.15em;margin-right:0.4em"></span>`#67b7a4` |
| `--syntax-attribute` | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#815f03;vertical-align:-0.15em;margin-right:0.4em"></span>`#815f03` | <span style="display:inline-block;width:0.95em;height:0.95em;border-radius:4px;border:1px solid var(--border);background:#fd8f3f;vertical-align:-0.15em;margin-right:0.4em"></span>`#fd8f3f` |

Two font tokens, `--font` and `--font-mono`, are shared by both themes. A
theme is a set of values for these tokens. A few colors are fixed rather than
taken from a token: the text of the current sidebar entry (white); the red
(`#d70015`) of diagram errors, comment errors, the problems bar and broken-link
marks; the yellow (`#ffd60a`, half seen through) of the
[words a search result found](../../search/README.md#landing-on-the-match). How the tokens reach highlighted code and diagrams is on
[How themes reach code and diagrams](../how-themes-reach-code-and-diagrams/README.md).

A [design](../../designs/README.md) has light and dark themes of its own: the
colors its `themes.yml` names, which it draws with instead of the tokens. It
is drawn in the theme of the page it is shown on, so a dark page shows a dark
design on a dark backdrop — see [Themes](../../designs/README.md#themes).
