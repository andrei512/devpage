---
page-type: reference
---
# Requirements

What the viewer must always do. Each requirement has its own page: the rule in
one sentence, why it exists, and how it is checked.

## Topics

- [devpage must never serve or read a file outside the folder it was started on](devpage-stays-inside-its-folder.md)
  — what devpage may serve, read and link to.
- [devpage must never change the files it serves](devpage-never-changes-served-files.md)
  — comments stay in `.devpage/`; a PDF or a video goes only where it was
  asked for.
- [A PDF must look the same as the page on screen](a-pdf-looks-like-the-page.md)
  — one renderer for screen and paper.
- [devpage must not do authentication itself](devpage-does-no-authentication.md)
  — identity comes from the proxy in front of it.
- [Widget names are unique](widget-names-are-unique.md)
  — widgets compose by name, so no name is shadowed.
- [The PNG of a design must look the same as the design on screen](a-design-png-looks-like-the-design.md)
  — the agent's PNG is the reader's canvas.
- [The time to draw a design must depend on what is on screen](drawing-time-depends-on-the-screen.md)
  — not on how many elements the design has.
- [In local mode, hot reload must work for every page](hot-reload-works-for-every-page.md)
  — a saved file shows in the open page, whatever made the page.
- [When a file is broken, the page must show the error](a-broken-file-shows-its-error.md)
  — a page never looks fine while a file behind it is broken.
