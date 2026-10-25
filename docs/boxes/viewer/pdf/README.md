# Printing to PDF

How a doc or a deck becomes a PDF that looks exactly like the page in the
browser: the `devpage pdf` command, the Chrome it drives, and what print mode
leaves out. It is a task and mechanism article under the [viewer](../README.md).

## `devpage pdf`

```sh
devpage pdf design/sync                 # the doc in design/sync/devpage.yml
devpage pdf talks/launch -o launch.pdf  # a deck, to a chosen file
devpage pdf design.md                   # one Markdown file, as a doc
```

It takes the same argument as `devpage` and writes `<title>.pdf` in the current
folder unless `-o` names the file; characters a file name can't hold — `/`,
`:`, `?` and the like — become `-`. It prints the file's name when it is done. It prints a **doc** — on A4 or Letter, with
its header and footer ([Writing a doc](../writing-a-doc/README.md#on-paper)) —
or a **deck**, one slide per 16:9 page
([Making slides](../making-slides/README.md#on-paper)). A site is many pages,
not one, and `devpage pdf` refuses it; print one of its pages from the browser
instead.

The browser's own print — ⌘P, *Save as PDF* — produces the same pages from an
open doc or deck, since both print the same print-mode page. `devpage pdf` adds
the doc's header and footer and needs no clicking, which is what a script or an
agent wants.

## The Chrome it drives

devpage does not render PDF itself. It starts its own server in the
background, which logs nothing, opens the document in print mode in a headless Chrome — laid out
as paper from the start, with the print media type, so every slide of a deck
is shown and a design embedded on any of them is fitted where it will print —
waits until
every diagram is drawn and every embedded design is painted — for at most 20
seconds, after which it prints whatever the page shows, so a design that never
paints (one with problems, say) costs that wait but doesn't stop the PDF; `devpage check`
is what reports it — and has Chrome print it — the same `Page.printToPDF`
call Playwright makes, spoken directly over the DevTools protocol by the
`chromiumoxide` crate, with no Node and no extra process.

Each run gives Chrome a profile folder of its own in the system's temporary
folder, so several prints can run at once, and deletes it when the run ends.

The Chrome is always **chrome-headless-shell**, never a Chrome installed on
the machine. devpage downloads it the first time it needs one — into
`~/Library/Caches/devpage/chrome-headless-shell` on macOS,
`~/.cache/devpage/chrome-headless-shell` elsewhere — and uses that copy from
then on. It is a bare program with no app bundle, so nothing devpage launches
appears in the macOS Dock, and every machine prints with the same browser.
The Chrome tests drive the same copy.

## Why the PDF matches the screen

Print mode is the same page, with the same Markdown, stylesheet, highlighting
and diagrams; it only leaves out live updates, comments, navigation, the
deck's controls and soft shadows (PDF viewers draw them as grey boxes), and
sets the page size
([A PDF is the page the browser shows](../constraints/a-pdf-is-the-page.md)).
A deck's slides are laid out at a fixed 1920 × 1080 and only scaled to the
window, so the printed page is the slide as it shows.
