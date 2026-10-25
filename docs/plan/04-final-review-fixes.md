---
id: "04"
blocked-by: []
boxes: [viewer]
---

# Widgets final review fixes

1. Widget tags inside indented code blocks and inside HTML comments are left
   alone (not expanded, not problems); the widgets page says what is skipped.
2. The paragraph comment icon stays on screen at every width (clamped to the
   window; inside the block's edge when there is no room to its right); the
   comments page describes the icon on the right.
3. An unknown block tag's error has the usual space below it.
4. Tables never widen the page on narrow screens (scroll inside their box).

## Acceptance criteria

- Tests: indented code with `Vec<String>` and `<Callout>` renders as plain
  code with no problem; `<!-- <Todo> -->` is no problem; a Chrome test at
  390px and 820px finds the comment icon inside the window and no horizontal
  page scroll.
- Gates green incl. `--ignored`; `devpage check docs` exit 0.
