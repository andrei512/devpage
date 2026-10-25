---
id: "01"
blocked-by: []
boxes: [viewer]
---

# The served folder, the document index and watching

Make the boundary, the index of document files and the watcher what the design
says, replacing the prototype's per-request scanning.

- The served folder is the current folder or `--root` (every command). Nothing
  opens or searches for `.git`. The entry document outside the served folder
  stops devpage with `<path> is outside the served folder <folder>`.
- An index of document files, built at start and updated by the watcher, is
  what serving, the nearest-site lookup and link checking use. No folder scans
  per request.
- The watcher skips a `.git` folder if present and every path any `.gitignore`
  in the served folder excludes (nested ones included); when the entry document
  is a site with a `watch` list, only matching paths refresh pages. The comment
  store is always watched.
- A site's `watch` field exists again (globs relative to the site's folder).
- Topics: an entry linking a document file opens that document, with no
  children; a link to a missing file or to a Markdown file outside the site's
  folder is left out and is a problem.
- A linked document file that is invalid is a problem shown on the pages that
  link to it, and renders as a page listing its problems.

## Acceptance criteria

- Route tests: a document file opens as its kind; a page shows in its nearest
  site's frame; a page in no site renders as a one-chapter doc; `/../x` and a
  symbolic link pointing outside get `404`.
- A test serves a folder with no `.git` and everything works; `--root` moves
  the boundary.
- A test shows a nested `.gitignore`d file and a file outside `watch` don't
  refresh, and a comment file always does.
- The mechanism pages (serving pages, live updates, navigation tree) and
  organizing navigation describe the code as built.
