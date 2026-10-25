# Tests

What devpage's test suite covers, module by module, how the tests are written,
and what has no test. It is a reference page under
[Developing devpage](../developing-devpage/README.md), for someone about to
change a module or wondering whether a behaviour is pinned.

<Terminal>

```text
$ cargo test                  # 91 tests; needs no network and no Chrome
$ cargo test -- --ignored     # the 20 tests that drive the installed Chrome
```

</Terminal>

## How they are written

Tests are written as `#[cfg(test)]` modules beside the code they test; the
`tests/` folder holds only fixtures — `tests/fixtures/sync-design`, a doc of
three chapters that the doc and PDF tests load. Route tests start the real router on a free port on
`127.0.0.1` and talk to it with `reqwest`; external-link tests point the
checker at a local web server, so the suite needs no network. The 20 tests
marked `#[ignore]` drive the installed Chrome through chromiumoxide — run them
with `--ignored` after changing the page script, the stylesheet, a widget,
print mode or `devpage pdf`.

Tests that need files write them into folders under the system temp
directory, named with the test process's id, and don't all remove them.

## By module

### `check`

*15 tests.*

- A site's navbar reported once and its links followed.
- A missing logo reported once.
- Widget problems — an unknown widget tag, a project widget with a system widget's name, a widget or a helper that doesn't compile, a widgets folder's problems reported once however many pages.
- Missing files and headings and links climbing out of the served folder.
- A slide number the deck doesn't have.
- A heading in a doc checked against the ids the doc renders.
- Topics checked against the page's own site, and a page in no site checked as a doc.
- A broken linked document named by its address, not an absolute path.
- Each external address asked once and every link to it reported.
- An unreachable address asked again before `devpage check` reports it.
- The preview's background check refreshing open pages.
- An unreachable address shown only after three failures in a row and cleared when it answers.
- The fixture doc, the devpage talk and devpage's own docs free of broken internal links.

### `comments`

*4 tests, 5 in Chrome.*

- A thread's whole life leaving `git status` clean.
- A thread file that doesn't parse named and left alone.
- The local author without a `.git` folder.
- Timestamps.

In Chrome:

- Commenting off until comment mode is on, a comment made, the switch remembered after a reload.
- Selected text becoming a thread in the margin and the store, then detaching.
- A paragraph comment on a nested list anchoring again after a reload.
- The comment icon and a wide table staying inside 390 px and 820 px windows.
- **C** on a deck.

### `document`

*8 tests.*

- Each kind parsed.
- A navbar read in order with its icons and dropdowns, `sidebar` and `theme-selector`, and its bad shapes refused.
- Bad kinds, fields and missing files rejected.
- A Markdown file as a one-chapter doc.
- The served folder as boundary and address root.
- A document outside it refused.
- A site's `watch` list.
- Only YAML with a devpage `kind` is a document file.

### `icons`

*1 test.*

- Lucide icons and GitHub's mark drawn.
- Unknown names known as such.

### `index`

*4 tests.*

- Document files found outside what `.gitignore` excludes.
- The nearest site above a page.
- Nested `.gitignore` files and `.git` folders out of sight.
- Changes keeping it current.

### `main`

*1 test.*

- Taking the next port when the first is busy.

### `navbar`

*6 tests.*

- Links, dropdowns, icons and icon-only links rendered in order.
- The link holding the page shown highlighted, a dropdown holding it too.
- Missing files and unknown icons as problems.
- No navbar rendering nothing, the comment-mode switch in the corner, and the theme selector off with `theme-selector: false` while the switch stays.
- A logo before the title and as the tab's icon, for a doc and a deck too.
- A missing logo as a problem and the page shown without it.

### `nav`

*3 tests.*

- Links and `###` groups read from the Topics section only.
- The tree stopping at a cycle.
- A Topics entry outside the site or to a missing file is a problem.

### `pdf`

*2 tests, 15 in Chrome.*

- The default file name made safe.
- A site refused.

In Chrome:

- The logo loaded at 22px before the navbar's title, centred on it, and the tab's icon.
- The navbar's dropdown by click and keyboard, its highlight following an in-place link.
- The theme selector following the system until clicked, then switching every token and remembering the theme across a reload.
- The navbar folding into a menu on a narrow screen.
- The fixture doc on A4 with its header and footer.
- The devpage talk one 16:9 page per slide.
- A deck's keys.
- A doc's outline above the text on a narrow screen.
- Every system widget rendering, long lines scrolling, task lists without bullets and an inline widget keeping its paragraph.
- Widgets in a doc and a deck, a page loading only the widgets it uses.
- Widgets mounting again after a live update and an edited widget or helper reaching open pages.
- A comment anchored inside a widget.
- A printed doc showing each widget's first render, terminal lines wrapping on paper.
- A broken helper's message and an unknown tag's error shown in place, and a project `Callout.tsx` leaving the system `Callout` rendering.

### `render`

*11 tests.*

- Headings never taking an id the frame uses.
- Every frame carrying the logo as its tab's icon, before the sidebar's and the outline's title.
- Raw HTML kept and Mermaid fences marked.
- Highlighting without a language label.
- Unknown languages left plain.
- Tables and heading ids.
- A doc's chapters with its outline.
- A deck's slides.
- Links kept for chapters and slides from other folders.
- A one-file doc's title.
- A page's title.

### `server`

*12 tests.*

- Widgets served compiled, for the document they belong to, with versioned helper imports.
- A page naming its widgets and carrying their placeholders, code left alone.
- A site's page printing as a doc of one chapter.
- `/` and the site's pages following the entry file's changes without a restart, through an invalid state and back.
- Comment threads over HTTP.
- Deployed writes needing the header and stamped with it, over HTTP and in the identity rule alone.
- A thread on a page outside the served folder refused.
- Serving a folder with no `.git` by what each file is.
- Nothing outside the served folder served.
- A served folder reached through a symbolic link.
- Each site's title linking to its own root page.

### `watch`

*7 tests.*

- A saved page refreshing open pages.
- Ignored files and files outside `watch` not refreshing.
- A comment thread always reaching its page.
- The `watch` list following the entry file.
- A site's `watch` narrowing only its own folder, whichever document devpage was started with.
- Widget files refreshing whatever `watch` says.

### `widgets`

*17 tests.*

- Widget tags expanded with Markdown content, expression, bare and self-closing props, code left alone — fenced, indented and inline code and HTML comments, but not indented list content.
- An unknown tag replaced by an error, not passed through.
- A `Tab` outside a `Tabs` reported at its page line.
- Nested and inline widgets, an inline widget at the start of a line staying in its paragraph.
- `Example`'s highlighted source.
- TSX compiled with the automatic runtime, widget shims, `__devpageUses` and versioned relative imports.
- A compile error's line.
- Compiles remembered until the source or version changes.
- A folder's version moving when a helper changes.
- Lowercase files as helpers and a system widget's name as a problem.
- A `Cards` icon drawn on the server and an unknown icon name as a problem.

## What has no test

<Callout kind="note" title="Not covered by any test">

syntax highlighting except through the `render` tests;
Chromium's download when no Chrome is installed (it needs a machine without
one); fullscreen on a deck (headless Chrome refuses it without a real user
gesture); and how the light and dark themes look, which is checked by eye.

</Callout>
