---
id: "03"
blocked-by: []
boxes: [viewer]
---

# Comments, production-quality

- A real comment input in the margin (and in the panel on narrow screens and on
  decks) — no `prompt()`. Reply and Resolve in the thread.
- One text model for quotes: the quote, prefix and suffix are taken and matched
  against the same text (text content, whitespace normalised), so nested lists
  and code anchor reliably; a paragraph comment quotes the block's text.
- Decks: **C** toggles the panel; comments never show in fullscreen unless
  toggled, never in print mode.
- Identity: local author from `git config user.name` (no `.git` needed; global
  config counts), else the system user name; deployed, `--user-header` (default
  `X-Forwarded-Email`), `401` on a write without it.

## Acceptance criteria

- Route tests: create, reply, resolve; deployed without the header → `401`;
  with it → the author is the header's value.
- A test runs a thread's whole life in a git repository and finds `git status`
  clean and tracked files unchanged.
- Ignored Chrome tests: select text → comment appears in the margin and in
  `.devpage/comments`; edit the quote away → it is detached; on a deck, **C**
  shows the panel.
- The comments page describes the code as built.
