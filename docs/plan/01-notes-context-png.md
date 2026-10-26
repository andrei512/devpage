# 01 — Notes, context, and a slide as a PNG

Read [the plan](README.md) first.

## What

1. **`<Notes>` on a slide.** A `<Notes>…</Notes>` block in a slide's `.md`
   holds that slide's speaker script. devpage finds it with the same rules it
   uses for widget tags (skips fenced, indented and inline code and HTML
   comments), takes it out of the rendered slide, and keeps its text (trimmed,
   as plain text) on the slide in the document model, so `devpage video` can
   read it. `Notes` is a name devpage owns: a project widget named `Notes` is
   a clash, reported the way a clash with a system widget is
   (one-name-one-widget). A second `<Notes>` on one slide, or `<Notes>` on a
   page that isn't a slide, is a problem shown on the page and failing
   `devpage check`.
2. **`context:` in a slides yml.** Optional string: what the talk is, who it's
   for, the tone. Kept on the document; unknown-key handling as for the other
   kinds' fields. Used by 02.
3. **`<deck>.yml/<n>.png`.** `GET` returns slide `n` (1-based) of the deck as a
   1920×1080 PNG, rendered by the browser devpage already drives
   (chrome-headless-shell), with devpage's controls hidden and the page ready
   (the same `READY` check `devpage pdf` uses). Out of range or not a deck:
   404 with a short reason. Follow how `<file>.design.png` is served
   (`src/designs.rs`) and the served-folder boundary.

## Docs (same commit as the code)

- `docs/boxes/viewer/making-slides/README.md`: a "Speaker notes" section
  (what `<Notes>` is for — the script a human or the narrator reads, never on
  the slide) and "A slide as an image" (the PNG URL, and why: the agent
  writing notes looks at the slide). Mention `context:`.
- Configuration reference: `context`.
- Glossary: notes, context.
- Widgets docs where system names are listed: `Notes` is reserved.
- Feature catalog (`docs/features/`): an entry for notes and slide PNGs.
- Do **not** touch `docs/talks/`.

## Tests

Notes parsing (inside code is ignored; removed from the HTML; text kept;
second Notes is a problem; Notes outside a slide is a problem; a project
`Notes` widget clashes), `context` parsing, the PNG route (a Chrome test,
`#[ignore]` like the other Chrome tests, checking 1920×1080 and the 404s).

## Done when

Gates green (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo test`, and the ignored Chrome tests you touched with
`cargo test -- --ignored <name>`), `devpage check docs` clean on internal
links, committed on your lane branch.
