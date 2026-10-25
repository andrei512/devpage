---
id: "04"
blocked-by: []
boxes: [viewer]
---

# Docs, slides and PDF, production-quality

- Doc: the outline moves above the text on a narrow screen; a one-file doc has
  no separate title; chapter ids are `<chapter>--<id>` with links rewritten.
- Slides: the keys in the design, fullscreen, `#n`, a slide's change swapping
  in place on the current slide; type scale tuned for 1920 × 1080.
- PDF: each launch's temporary Chrome profile is deleted after the run; the
  default file name is the title with characters illegal in file names
  replaced; the Chromium download path is used only when no Chrome is found and
  says so; `devpage pdf` on a site stops with the design's error.
- `--host` prints the address it was given.

## Acceptance criteria

- Ignored Chrome tests: the example doc prints on A4 with its header and
  footer and page numbers; the example deck prints one 16:9 page per slide.
- Unit tests for the file-name sanitising and the site refusal.
- The writing-a-doc, making-slides and PDF pages describe the code as built.
