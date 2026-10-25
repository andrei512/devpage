---
id: "08"
blocked-by: []
boxes: [viewer]
---

# Second drift review fixes

1. `devpage pdf <file.md>` prints that file as a one-chapter doc in print mode,
   whatever site holds it (the design: "one Markdown file, as a doc").
2. Topics entries are checked against the page's nearest site, in
   `devpage check` and the preview alike, for every site reached — not only
   when the entry is a site.
3. A `#fragment` on a link to a doc is checked against the ids the doc renders
   (`<chapter>--<id>`, and the chapter ids); writing-a-doc says how to link to
   a heading in a doc from outside it.
4. Wording: comments page and `comments.rs` say the *server* writes nothing
   else; the viewer README's served-folder summary drops "or written".
5. Notes made true: the configuration reference lists `chapters lists no
   files` / `slides lists no files`; serving-pages states what an unreadable
   `.md` answers (or the code answers as documented); the thread id's suffix is
   described as it is generated; `devpage pdf` does not print its internal
   server's request log.

## Acceptance criteria

- Tests: printing a Markdown file inside a site uses the doc's print mode (no
  site nav; doc page size); a sibling site's Topics entry inside its own folder
  is no problem and one outside it is; `doc.yml#nosuch` is a problem and
  `doc.yml#c1--real` is not.
- Gates green, `cargo test -- --ignored` green, `devpage check docs` exit 0.
