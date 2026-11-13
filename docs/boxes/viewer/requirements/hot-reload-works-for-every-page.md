---
page-type: constraint
id: hot-reload-works-for-every-page
---
# In local mode, hot reload must work for every page

<Callout kind="note" title="The rule">

In local mode, hot reload must work for every page. When a file a page is made
from is saved, the open page shows the change without a reload by hand —
whether the page comes from Markdown, a widget, a design or an extension.

</Callout>

## Why

An author and an agent work on a page while looking at it. A page that shows
the change for one kind of content and not for another makes the reader doubt
all of it, and reload by hand to be sure.

## Check

A core test saves a page and finds the refresh sent to open pages. A browser
test saves a file and finds the widgets mounted again in the open page. The
extension test narrows the site's `watch` to one page, still finds a new file
under an extension's input, then edits a file and requires a new preparation
before the refresh. An extension's files are watched by devpage's own watcher;
no extension starts a watcher or polls.
