# Tests

What devpage's test suite covers, module by module, how the tests are written,
and what has no test. It is a reference page under
[Developing devpage](../developing-devpage/README.md), for someone about to
change a module or wondering whether a behaviour is pinned.

<Terminal>

```text
$ cargo test                  # 178 tests in the four crates; needs no network and no Chrome
$ cargo test -- --ignored     # the 46 tests that drive chrome-headless-shell
```

</Terminal>

## How they are written

`cargo test` runs every crate in the workspace: 113 in `devpage`, 59 in the
engine `devpage-design`, 4 in `devpage-design-wasm`, whose tests run
natively, and 2 in `devpage-design-module`. Tests are written as
`#[cfg(test)]` modules beside the code they test — the canvas's Chrome tests
in `src/designs/canvas_tests.rs`, a child module of `designs`; the `tests/`
folder holds only fixtures — `tests/fixtures/sync-design`, a doc of three
chapters that the doc and PDF tests load. Route tests start the real router on
a free port on `127.0.0.1` and talk to it with `reqwest`; external-link tests
point the checker at a local web server, so the suite needs no network.

The 46 tests marked `#[ignore]` drive chrome-headless-shell through
chromiumoxide — the copy
[`devpage pdf` downloads](../../viewer/pdf/README.md#the-chrome-it-drives)
into devpage's cache folder, fetched by the first test that needs it — never
a Chrome installed on the machine, so a run puts nothing in the macOS Dock.
They run in parallel. Run them with `--ignored` after changing the page
script, the design script, the module, the stylesheet, a widget, print mode
or `devpage pdf`. The canvas tests start it with `--enable-unsafe-webgpu` and
`--enable-gpu`: the shell keeps its GPU process off without the second, and
then a screenshot holds black where a WebGPU canvas is; with it, WebGPU gets
the machine's real adapter and the screenshot is the canvas as a reader sees
it.

Tests that need files write them into folders under the system temp
directory, named with the test process's id, and don't all remove them.

### The watcher's tests need file-system events

Nine tests save a file and wait for devpage to hear of it through `notify`:

- six `watch` tests — `a_saved_page_refreshes_open_pages`,
  `a_comment_thread_always_reaches_its_page`,
  `the_watch_list_follows_the_entry_file`,
  `a_sites_watch_list_narrows_only_its_own_folder`,
  `the_entrys_watch_list_does_not_quiet_another_folder` and
  `widget_files_refresh_whatever_watch_says` (of the other two,
  `ignored_files_and_files_outside_watch_do_not_refresh` expects no event and
  `a_changed_file_reaches_the_design_store` waits for none, so both pass
  either way);
- `server::the_entry_document_follows_its_file_without_a_restart`;
- two Chrome tests in `comments::browser` —
  `selected_text_becomes_a_thread_in_the_margin_and_in_the_store` and
  `a_live_update_with_the_comments_shown_swaps_the_page_in_place`.

Where the platform delivers no events to the test process — `notify` itself
receives nothing, as can happen in some sandboxed or restricted sessions on
macOS — those tests fail while everything else passes. That says nothing
about the change under test; run them where file events work before calling
the suite green.

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

*4 tests, 6 in Chrome.*

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
- A live update with the comments shown swapping the page in place rather than reloading it.

### `designs`

*17 tests, 25 in Chrome.*

- The store announcing only the designs made from a changed file, and resolving a design again when a missing import appears; a save landing while a design resolves never leaving a stale scene kept.
- An image that doesn't decode a problem at its `src` in what `devpage check` reads and in the PNG.
- A drawing too big to paint a problem in the store and in what `devpage check` reads.
- A real file named like a design's PNG served as it is; a link to a design's PNG checked as the design.
- No design file — a pasted zero-width space in a colour, deep nesting, a huge number, a truncated file — stopping the store, which still sees saves afterwards.
- Nothing outside the served folder read.
- A PNG, or `422` and the problems, and `404` for a design outside the served folder or missing.
- A design as the entry document; the canvas page titled without `.design`.
- The design service answering only `Watch`.
- The watch stream over gRPC-Web sending the scene, a patch of only what changed, nothing for a no-op, problems, and a deleted design coming back when written again.
- One stream over several designs sending each scene once, patches naming their design, going on after one is deleted, and bringing it back when it is written again.
- `devpage check` naming a broken embedded design's `file:line:col`.

In Chrome:

- Embeds inside widgets on a page in a subfolder — in a `Callout`, a `Tab`, a `Steps` item and a `Callout` inside a `Steps` item — each finding its design beside the page.
- The canvas on the GPU and on the CPU each matching the PNG by the engine's comparison, over the whole drawing and a margin of plain page around it.
- A click navigating, back returning, an overlay opening and closing; an interaction inside an instance from another folder opening its own target.
- The wheel zooming about the pointer and panning.
- An undone typo clearing the problems.
- A deleted design, on its canvas page and embedded, coming back when written again — after a page refetch too — without reopening its stream meanwhile.
- A canvas saying so when the engine doesn't load.
- An embedded design drawing at its own size and surviving a page edit; centred at its own size and only ever scaled down, to the page's width or the window's height.
- The backdrop's link opening the design it shows on its canvas page; a small design not enlarged on its canvas page.
- A doc printing its embedded design; a doc with eight embeds printing as fast as one with one.
- Eight embedded designs painting through one stream while the page still loads; leaving the last design in place closing the stream.
- An embed with problems surviving a page refetch.
- The canvas page's light/dark switch repainting what a design leaves transparent and remembered across a reload, and leaving the problems bar whole in light and dark; an embedded design painting again when the page switches theme.
- A design inside a project widget on a slide not yet shown drawing once shown, inside the slide, and printing promptly.
- An embed inside a widget keeping the design it navigated to, and its way back, across a page refetch.
- A design on a slide scaled down to a 1280 × 720 or 1600 × 900 window painted whole on the GPU and on the CPU, each corner its own color, and a click landing where the reader sees it.

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

*15 tests.*

- An image of a design becoming its canvas, and a raw `<img>` found whole with or without its closing slash, its attributes read however they are quoted, spaced or capitalised.
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

*8 tests.*

- A saved page refreshing open pages.
- Ignored files and files outside `watch` not refreshing.
- A comment thread always reaching its page.
- The `watch` list following the entry file.
- A site's `watch` narrowing only its own folder, whichever document devpage was started with.
- Widget files refreshing whatever `watch` says.
- A changed file reaching the design store, and none under `.devpage/`, handed over directly, without waiting on file-system events.

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

## The design engine's crates

### `devpage-design`

*59 tests.*

- `parse`, 8: imports, attributes and text with props; JSX's strings without escapes, bare attributes and names with `-`; nesting and numbers kept within their limits, arrays and objects counting as levels and numbers too small named so; JSX whitespace in text; every syntax error, and each problem, at its line and column.
- `resolve`, 23: props and defaults; an import name unique and never a built-in element's; sizes and spacing never negative; `justify` naming all it takes; `x` and `y` in a frame with auto layout, a negative size, an attribute given twice and an import of something other than a design each a problem; `props` only on the root, and no `x` or `y` on it; `"fill"` only under auto layout; a colour with anything but hex digits in it a problem, not a panic; a tree nested too deep through instances; an instance taking its props and the attributes every element takes; values parsed into their types; opacity kept between 0 and 1; every problem reported at once and each at its place; a missing import a problem at its line, used or not; a design that uses itself; paths kept inside the served folder; an image that doesn't decode — a broken PNG, a truncated JPEG — a problem at its `src`, and a good one's pixels going with the scene; `clip` only on a frame; numbers inside a gradient, a `viewBox` and a `d` held to the same limits as any other; an interaction relative to the file that wrote it, one to nowhere a problem, and one only ever to a design; a valid design having none.
- `render`, 27: auto layout, hug, fill, a root that fills on its own hugging, a drawing laid out once, align and justify, and absolute placement; a hugging frame with no layout taking its children's extent, nested ones too; the drawing being the root plus what is painted outside it, cut by clipping, and a drawing too big being a problem; no input — hostile values for every attribute of every element, a file cut at every character — making the engine panic; text hugging and centered; fills, gradients along their angle, strokes inside the box, per-corner radius, a shadow following each corner's radius and the spread, sharp and blurred, clip and opacity as one group; vectors scaled into and hugging their view box; images covering their box, PNG and JPEG decoding; the example designs painting as their reference PNGs; the comparison forgiving anti-aliased edges only, and catching a missing divider or icon on the checkout design.
- `wire`, 1: a patch carrying only the nodes that changed.

### `devpage-design-wasm`

*4 tests.*

- A scene sized, painted and hit-tested.
- A patch before any scene, or garbage, doing nothing.
- Problems keeping the scene until a patch clears them.
- A click landing only on what a clip shows.

### `devpage-design-module`

*2 tests.*

- The vendored module built from the sources as they are.
- The fingerprint changing with an engine source, the `wasm` profile and a locked version in the module's tree, but not with a package outside it.

## What has no test

<Callout kind="note" title="Not covered by any test">

syntax highlighting except through the `render` tests; a canvas reopening
its stream after devpage restarts, closing streams in a hidden tab, and
drag-to-pan and pinch on the canvas; chrome-headless-shell's first download
(it needs an empty cache folder); fullscreen on a deck (headless Chrome refuses it without a real user
gesture); and how the light and dark themes look, which is checked by eye.

</Callout>
