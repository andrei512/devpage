---
id: "09"
blocked-by: []
boxes: [viewer]
---

# Third drift review fixes

1. A page in no site is checked as a doc of one chapter, whatever document the
   check started from — exactly as the preview shows it. A repository README
   with a Topics section, linked from a docs site, is no problem.
2. serving-pages says what is true: `devpage pdf` uses `server::printing`,
   which builds its own index and logs nothing; `server::router` exists only
   for tests; request logging is described as the served router's.
3. Drop `pdf::doc_params`'s unused `title` parameter.

## Acceptance criteria

- A test: a site page links to `../README.md`, whose Topics names a file
  beside it; `devpage check` on the site reports nothing for it.
- Gates green, `cargo test -- --ignored` green, `devpage check docs` exit 0.
