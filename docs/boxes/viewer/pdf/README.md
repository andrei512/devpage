---
page-type: task
---
# Printing to PDF

How a doc or a deck becomes a PDF that looks exactly like the page in the
browser: the `devpage pdf` command and what print mode leaves out. It sits under the [viewer](../README.md); how the PDF is made is on
[How a PDF is printed](how-a-pdf-is-printed/README.md).

## `devpage pdf`

<Terminal>

```
$ devpage pdf design/sync
Sync design.pdf
$ devpage pdf talks/launch -o launch.pdf
launch.pdf
```

</Terminal>

The first prints the doc in `design/sync/devpage.yml`, the second a deck to a
chosen file; `devpage pdf design.md` prints one Markdown file as a doc.

It takes the same argument as `devpage` and writes `<title>.pdf` in the current
folder unless `-o` names the file; characters a file name can't hold — `/`,
`:`, `?` and the like — become `-`. It prints the file's name when it is done. It prints a **doc** — on A4 or Letter, with
its header and footer ([Writing a doc](../writing-a-doc/README.md#on-paper)) —
or a **deck**, one slide per 16:9 page
([Making slides](../making-slides/README.md#on-paper)). A site is many pages,
not one, and `devpage pdf` refuses it; print one of its pages from the browser
instead. It refuses a design too; a page that embeds the design prints it, and
its [PNG](../designs/viewing-a-design/README.md#looking-at-a-design-as-a-png) is its picture.

The browser's own print — <Keys>⌘ P</Keys>, *Save as PDF* — produces the same pages from an
open doc or deck, since both print the same print-mode page. `devpage pdf` adds
the doc's header and footer and needs no clicking, which is what a script or an
agent wants.

## Why the PDF matches the screen

Print mode is the same page; it only leaves things out, and sets the page size
([A PDF must look the same as the page on screen](../requirements/a-pdf-looks-like-the-page.md)):

| Kept on paper | Left out |
| --- | --- |
| the Markdown, the stylesheet | live updates |
| highlighted code | comments |
| diagrams, and widgets (their first render) | navigation and the deck's controls |
| the doc's header and footer | soft shadows (PDF viewers draw them as grey boxes) |

A deck's slides are laid out at a fixed 1920 × 1080 and only scaled to the
window, so the printed page is the slide as it shows.

## Topics

- [How a PDF is printed](how-a-pdf-is-printed/README.md) — the server
  `devpage pdf` starts, what it waits for, and the pinned Chrome it drives.
