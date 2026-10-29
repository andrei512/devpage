# Tests

What devpage's test suite covers, module by module, how the tests are written,
and what has no test. It is a reference page under
[Developing devpage](../developing-devpage/README.md), for someone about to
change a module or wondering whether a behaviour is pinned.

<Terminal>

```text
$ cargo test                  # 515 tests in the six crates; needs no network and no Chrome
$ cargo test -- --ignored     # the 107 tests that drive Chrome or Docker, call GPT-Live, or time the engine
```

</Terminal>

## How they are written

`cargo test` runs every crate in the workspace: 169 in `devpage`, 182 in its
core `devpage-core`, 140 in the engine `devpage-design`, 9 in
`devpage-design-wasm`, whose tests run natively, 2 in
`devpage-design-module` and 13 in `devpage-bench`. Tests are written as `#[cfg(test)]` modules beside
the code they test — the canvas's Chrome tests in
`src/drive/server/canvas_tests.rs` and the comments' in
`src/drive/server/comments_browser.rs`, child modules of `server`. The `tests/`
folder holds three, [the search's quality](#the-searchs-quality),
[a server's threads](#a-servers-threads) and
[the restore of a deploy](#the-restore-of-a-deploy), and fixtures — `tests/fixtures/sync-design`, a doc of three
chapters that the doc and PDF tests load, `tests/fixtures/narrated-talk`,
a deck of three slides with notes that the live video test records, and
`tests/fixtures/search-docs`, a frozen copy of devpage's docs that the
search's quality is measured on. Route tests start the real router on
a free port on `127.0.0.1` and talk to it with `reqwest`. The core's
external-link tests ask a scripted table through the checker's `Ask` port,
and the HTTP adapter's point it at a local web server, so the suite needs no
network.

Of the 107 tests marked `#[ignore]`, 98 drive Chrome, and one of them also
calls GPT-Live; two run the server deploy example in Docker, one at a time,
since they share its compose project; six time the engine in a release build —
a save to its patch in the store at 1,000, 10,000 and 100,000 rows, what an
edit to one row of a column costs a drawing at those lengths, an edit in every
kind of frame at those lengths, how an edit's update grows to 200,000
siblings, one update and its undo over and over for a profiler, and matching
rewritten lists; the last asks
Google's server for the pinned Chrome's zips, all at once, so it needs the
network.
All but the two `video` tests drive chrome-headless-shell through
chromiumoxide — the copy
[`devpage pdf` downloads](../../viewer/pdf/README.md#the-chrome-it-drives)
into devpage's cache folder, fetched by the first test that needs it — never
a Chrome installed on the machine, so a run puts nothing in the macOS Dock.
They run in parallel, so the longest bounds the run: the `video` test that
records a deck, which takes as long as its slides play. Run them with
`--ignored` after changing the page
script, the design script, the module, the stylesheet, a widget, print mode,
`devpage pdf` or a slide's PNG. The two `video` tests drive Chrome for
Testing instead — the copy [`devpage video` downloads](../../viewer/making-a-video/README.md#the-chrome-it-drives),
fetched the same way; run them after changing `devpage video` or its
recorder, `assets/video.js`. The live one records
`tests/fixtures/narrated-talk` with GPT-Live, which bills per minute, and
passes without doing anything unless `DEVPAGE_OPENAI_API_KEY` is set. The canvas tests and a slide's PNG start it
with `--enable-unsafe-webgpu` and `--enable-gpu`: the shell keeps its GPU process off without the second, and
then a screenshot holds black where a WebGPU canvas is; with it, WebGPU gets
the machine's real adapter and the screenshot is the canvas as a reader sees
it.

Tests that need files write them into folders under the system temp
directory, named with the test process's id, and don't all remove them.

### The watcher's tests need file-system events

Twenty tests save a file and wait for devpage to hear of it through the
platform's watcher:

- eight `adapters::watch` tests that count what the platform reports —
  `writes_out_of_sight_are_never_reported`,
  `writes_in_sight_are_reported_shallow_and_deep`,
  `a_new_gitignore_rule_stops_a_folders_events`,
  `a_nested_build_folder_stops_its_events_once_made`,
  `a_folder_deleted_and_made_again_in_one_burst_is_watched_again`,
  `a_deep_folder_deleted_and_made_again_is_watched_again`,
  `the_comment_store_is_reported_and_the_rest_of_devpage_is_not` and
  `an_ignored_folder_a_watch_pattern_names_is_watched`;
- seven `adapters::watch` tests — `a_saved_page_refreshes_open_pages`,
  `a_comment_thread_always_reaches_its_page`,
  `a_thread_written_through_the_store_is_announced_once_and_a_hand_edit_once`,
  `the_watch_list_follows_the_entry_file`,
  `a_sites_watch_list_narrows_only_its_own_folder`,
  `the_entrys_watch_list_does_not_quiet_another_folder` and
  `widget_files_refresh_whatever_watch_says` (of the others,
  `ignored_files_and_files_outside_watch_do_not_refresh` expects no event,
  and the rest feed made-up events, so they pass either way);
- one macOS test, `a_save_just_before_a_folder_leaves_its_stream_is_heard`:
  a file saved and its folder passed from an FSEvents stream to kqueue at
  once, as the first comment's `.devpage/` does to the served folder, still
  reported;
- two `drive::server` tests —
  `the_entry_document_follows_its_file_without_a_restart` and
  `a_server_and_the_watcher_its_options_start_announce_a_write_once`;
- two Chrome tests in `drive::server::comments_browser` —
  `selected_text_becomes_a_thread_in_the_margin_and_in_the_store` and
  `a_live_update_with_the_comments_shown_swaps_the_page_in_place`.

A test that expects a message goes on the moment it arrives, waiting up to
10 s for it; one that expects none listens for 1.5 s, long enough for any the
write would cause.

Where the platform delivers no events to the test process — the watcher
itself receives nothing, as can happen in some sandboxed or restricted sessions on
macOS — those tests fail while everything else passes. That says nothing
about the change under test; run them where file events work before calling
the suite green.

## By module

`devpage-core`'s modules by their names; the binary's by their paths under
`adapters::` (what devpage's core is wired to) and `drive::` (what drives it).

### `addresses`

*3 tests.*

- Every built-in asset at its content address, the hash of its bytes; its bare name and a stale hash finding it, not as current.
- An asset naming the others by their content addresses.
- A system widget's address carrying its own version.

### `adapters::changes`

*4 tests.*

- Every subscriber hearing every change, in order.
- One that falls behind hearing a refresh rather than losing an update.
- `heard` reporting any change since the last look, without waiting.
- `next` waiting for a change and ending when the bus is gone.

### `adapters::http`

*2 tests, against a local web server.*

- `devpage check` asking each external address once and reporting every link to it.
- The preview checking in the background and refreshing open pages.

### `adapters::sqlite`

*3 tests; its threads are tested through `drive::server_mode`.*

- A version number given once: a second publish that read the same next number refused, and a deleted version's number never given again.
- Deleting the latest version making the one before it latest, and deleting the only one deleting the document.
- Deleting a document deleting its versions and nothing else.

### `cache`

*2 tests.*

- The least recently read versions going, oldest first, until the cache is at 90% of its limit; nothing going under it.

### `changes`

*1 test.*

- The page reading the same `change` and `comments /<page>` messages as before the bus was typed.

### `check`

*17 tests.*

- `<Notes>` on a page that isn't a slide, a second one on a slide, and one without its `</Notes>`, as problems — in `devpage check`, however a slide is reached, and in the preview.
- A site's navbar reported once and its links followed.
- A missing logo reported once.
- Widget problems — an unknown widget tag, a project widget with a system widget's name, a widget or a helper that doesn't compile, a widgets folder's problems reported once however many pages.
- Missing files and headings and links climbing out of the served folder.
- A diagram that doesn't parse reported at its fence's line with pele's message, past front matter, in `devpage check` and in the preview; one in a `markdown` fence never.
- A slide number the deck doesn't have.
- A heading in a doc checked against the ids the doc renders.
- Topics checked against the page's own site, and a page in no site checked as a doc.
- A broken linked document named by its address, not an absolute path.
- The preview reporting each problem once to its reporter.
- A preview of files that never change checking each page once, the same problems on every later view; one of files that change checking on every view.
- The fixture doc, the devpage talk and devpage's own docs free of broken internal links.

### `comments`

*13 tests — 4 of the port, 4 of its YAML adapter, 5 of re-anchoring; 6 in Chrome, in `drive::server::comments_browser`, and 1 on a published document.*

- A page that climbs out of the served folder and a made-up id refused.
- Every write announcing its page once, and a failed one nothing.
- The local author without a `.git` folder.
- Timestamps.
- A thread's whole life leaving `git status` clean.
- A thread file exactly the YAML it always was.
- A thread file that doesn't parse named and left alone.
- Only a change the store didn't make itself left for the watcher to announce.
- A page's text read as the page script reads it — tags gone, entities decoded, white space squeezed, scripts and styles left out; a quote with its context keeping its anchor, one whose context changed taking its new context, counted in characters, and one that is gone detached.

In Chrome:

- Commenting off until comment mode is on, a comment made, the switch remembered after a reload.
- Selected text becoming a thread in the margin and the store, then detaching.
- A paragraph comment on a nested list anchoring again after a reload.
- The comment icon and a wide table staying inside 390 px and 820 px windows.
- **C** on a deck.
- A live update with the comments shown swapping the page in place rather than reloading it.

### `designs`

*17 tests in the store; 15 more through the server, and 44 in Chrome, in `drive::server`.*

- Saves through the store — rows inserted at the front and the end, edited and removed, a component and a theme changed, a break and its fix — each sent as the store's one patch, which brings the browser's copy to the store's scene, nothing resolved whole after the first resolve, and a stream joining late sent the scene whole.
- The store announcing only the designs made from a changed file, and resolving a design again when a missing import appears; a save landing while a design resolves never leaving a stale scene kept.
- A change to a font file, or to any file in a font folder, resolving again only the design set in it; a folder with two regular faces a problem at its `fonts` entry in the `themes.yml`, in the store and in what `devpage check` reads.
- An image that doesn't decode a problem at its `src` in what `devpage check` reads and in the PNG.
- A design 100,000 pixels tall drawable in the store and in what `devpage check` reads, its PNG streamed band by band from the store's tile cache, which never holds more than its 64 MB; a PNG asked for again painting no tile, and after an edit to one row only the tiles over it — undone, those again, and the PNG as before; each theme of a themed design keeping its own tiles, and a design that stops using its themes dropping the dark ones; a writer that fails stopping the painting within the first band; a save landing while a PNG's reader has stopped reading, the PNG then ending rather than finishing from the next scene; a design too wide for a row of tiles in one band banded in strips, its PNG right and nothing cached; images too many for the GPU's atlas a problem at the `src` of the first that doesn't fit.
- An image file decoded once per read of it: a save of the design showing it doesn't decode it again, a save of the file does — the scene keeping the decode it has while the bytes are the same.
- A real file named like a design's PNG served as it is; a link to a design's PNG checked as the design.
- No design file — a pasted zero-width space in a colour, deep nesting, a huge number, a truncated file — stopping the store, which still sees saves afterwards.
- Nothing outside the served folder read.
- A design's PNG answering `304` from its scene without painting, until a design it uses changes.
- A PNG, or `422` and the problems, and `404` for a design outside the served folder or missing.
- A design as the entry document; the canvas page titled without `.design`.
- The design service answering only `Watch`.
- Two streams sending the one patch the store made, and a third joining later the scene whole; a change that only makes a scene themed still sent.
- The watch stream over gRPC-Web sending the scene, a patch of only what changed, nothing for a no-op, problems, and a deleted design coming back when written again.
- One stream over several designs sending each scene once, patches naming their design, going on after one is deleted, and bringing it back when it is written again.
- `devpage check` naming a broken embedded design's `file:line:col`.

In Chrome:

- Embeds inside widgets on a page in a subfolder — in a `Callout`, a `Tab`, a `Steps` item and a `Callout` inside a `Steps` item — each finding its design beside the page.
- The canvas on the GPU and on the CPU each matching the PNG by the engine's comparison, over the whole drawing and a margin of plain page around it.
- A design mixing families — a variable font at weights from 300 to 900, a folder of static faces, a paragraph that wraps, and the embedded Inter — on the GPU and on the CPU, matching the PNG the same way; the examples, mixing Lora and Figtree, too.
- On the GPU, as in the PNG: a file wider than the GPU's atlas, twelve distinct 2000-pixel files together, and a save swapping six of them for new ones; shadows drawn in pieces — square, sampled and disc corners, a pill, a thin box, ellipses in wedges.
- A navigation from a design whose four 4,096-pixel images fill the GPU's atlas to one with an image of its own drawing the second as its PNG, on a canvas page and inside an embed.
- A click navigating, back returning, an overlay opening and closing; an interaction inside an instance from another folder opening its own target.
- The wheel zooming about the pointer and panning.
- A design 100,000 pixels tall at 100% painting only the tiles of its view, on the GPU and the CPU; a pan over it painting no tile and costing a frame no more than on a 400 × 300 design.
- Zoomed in 4× into the checkout, the GPU and the CPU each showing what the engine paints there at 4×, and agreeing.
- An edit to one row of the 100,000-pixel design repainting only the tile it touches, and one out of view none, on the GPU and the CPU.
- A canvas without WebGPU painting when its scene reaches the worker before the worker has loaded the engine; asking the worker for the tile under the view's middle first, and never more than the frame's budget at once.
- An undone typo clearing the problems.
- A deleted design, on its canvas page and embedded, coming back when written again — after a page refetch too — without reopening its stream meanwhile.
- A canvas saying so when the engine doesn't load.
- An embedded design drawing at its own size and surviving a page edit; centred at its own size and only ever scaled down, to the page's width or the window's height.
- The backdrop's link opening the design it shows on its canvas page; a small design not enlarged on its canvas page.
- A doc printing its embedded design; a doc with eight embeds printing as fast as one with one.
- Eight embedded designs painting through one stream while the page still loads; leaving the last design in place closing the stream.
- An embed with problems surviving a page refetch; a swap keeping a design outside a widget's canvas and dropping a removed widget's design from the stream.
- The canvas page's light/dark switch repainting what a design leaves transparent and remembered across a reload, and leaving the problems bar whole in light and dark; an embedded design painting again when the page switches theme.
- A design inside a project widget on a slide not yet shown drawing once shown, inside the slide, and printing promptly.
- An embed inside a widget keeping the design it navigated to, and its way back, across a page refetch.
- A design on a slide scaled down to a 1280 × 720 or 1600 × 900 window painted whole on the GPU and on the CPU, each corner its own color, and a click landing where the reader sees it.

### `diagrams`

*6 tests.*

- A flowchart drawn to SVG with its `click` links, its tooltip as their title, an `https://` target kept through the engine's `URL`, and its colors left as `--pele-*` variables.
- pele's error as the diagram's — a parse error, no diagram type — and the engine drawing the next one.
- A diagram drawn before, or one pele rejected, coming from the cache; only new ones sent to the thread.
- A diagram past the time limit failing with how long it may take, and not kept, so it is tried again.
- A diagram past the memory limit failing as out of memory, and the next one drawn in a fresh runtime.
- Fences in rendered HTML replaced by their drawings or by the error block with the source, escaped; other code left alone.

### `document`

*9 tests.*

- Each kind parsed.
- A deck's audience, goal, tone and pronounce, each slide's notes and delivery; those fields refused on the other kinds, and `context` refused.
- A navbar read in order with its icons and dropdowns, `sidebar` and `theme-selector`, and its bad shapes refused.
- Bad kinds, fields and missing files rejected.
- A Markdown file as a one-chapter doc.
- The served folder as boundary and address root.
- A document outside it refused.
- A site's `watch` list.
- Only YAML with a devpage `kind` is a document file.

### `external`

*3 tests, against a scripted `Ask`.*

- `devpage check` asking an unreachable address once more before reporting it, and a broken one once.
- Outside a runtime, the preview asking nothing.
- The preview showing an unreachable address only after three failures in a row, and clearing it when it answers.

### `html`

*4 tests, on strings.*

- Text escaped for an element's text and a quoted attribute, and back.
- A tag's attributes read however they are quoted, spaced or capitalised.
- Every value of one attribute found in tags only — never in a comment, escaped code or text, or under another name such as `xlink:href`.
- An address on the web, with another scheme, or a file with its fragment — a colon in a path or a fragment being no scheme.

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

### `markup`

*6 tests, on strings.*

- `<Notes>` found with its line, none in code or comments; its text kept without its indentation; its `delivery` kept; one whose attributes can't be read keeping its text and no delivery; an unclosed one running to the end of the page.

### `navbar`

*7 tests.*

- Links, dropdowns, icons and icon-only links rendered in order.
- The link holding the page shown highlighted, a dropdown holding it too.
- Missing files and unknown icons as problems.
- A target outside the served folder a problem, followed neither as a file nor as an address on the web.
- No navbar rendering nothing, the comment-mode switch in the corner, and the theme selector off with `theme-selector: false` while the switch stays; with comments off, no switch.
- A logo before the title and as the tab's icon, for a doc and a deck too.
- A missing logo as a problem and the page shown without it.

### `nav`

*4 tests.*

- Links and `###` groups read from the Topics section only.
- The tree stopping at a cycle.
- A Topics entry outside the site or to a missing file is a problem.

### `drive::api_types`

*1 test.*

- Every refusal the server answers — not found, unauthorized, unprocessable, a failure — read back by the client as the same refusal.

### `drive::client`

*17 tests. Every server command runs against a real `devpage server` on a `file://` bucket, with each request it hears recorded.*

- The zip holding exactly what the check follows — the document, its pages, linked images, a design and its import, the logo, the project widgets and their imports — and not a draft or a secret in the same folder; a link beside the document's folder widening the zip to hold it.
- A broken link stopping `publish` before any request, in text and `--json`.
- `publish` remembering the document per context in `published.yml`, adding a version the second time, the bucket keeping the zip it sent and its link rendering the new version; `--id` publishing to that document, `--new` creating one, the two refused together, and an id the server doesn't have pointing to `--new`.
- Contexts added, used and listed, the current one marked; `DEVPAGE_CONTEXT`; no context saying how to add one; a token command's output sent as the bearer token and stored nowhere.
- `list`, `versions` and `comments` printing what the server answers, their `--json` the server's own JSON unchanged; a detached thread marked with the version that detached it, not the latest; `reply` and `resolve` reaching the thread; a design's `download` its PNG; `delete` of a version and of the document; an unknown document the server's not-found refusal, naming the server.
- The threads as text: grouped by page, each thread's quote in its context, its messages with their authors aligned; every command parsing as the reference writes it.

### `drive::pdf`

*4 tests, 35 in Chrome, 1 on the network.*

- The default file name made safe.
- A site refused.
- The pinned Chrome and Chrome for Testing already in a cache folder used as they are, with no network call.
- A failed download naming the browser, its version, the platform and the address.

On the network:

- Both browsers' zips at the pinned version answering `200` for every platform.

In Chrome:

- The logo loaded at 22px before the navbar's title, centred on it, and the tab's icon.
- The navbar's dropdown by click and keyboard, its highlight following an in-place link.
- The theme selector following the system until clicked, then switching every token and remembering the theme across a reload.
- A diagram in the page's HTML as served, its shapes taking the theme's `--surface` in light and again in dark after the switch, and its node's link opening the page in place.
- The navbar folding into a menu on a narrow screen.
- The fixture doc on A4 with its header and footer.
- A doc embedding a design with problems printed without waiting for it.
- The devpage talk one 16:9 page per slide.
- A deck's keys; a slide change, and back, fetching nothing with a slide shown in every frame.
- A doc's outline click scrolling without a fetch.
- A deck served with comments off showing no comment UI, even with comment mode remembered on, and <Keys>C</Keys> doing nothing.
- A doc's outline above the text on a narrow screen.
- Every system widget rendering, long lines scrolling, task lists without bullets and an inline widget keeping its paragraph.
- Widgets in a doc and a deck, a page loading only the widgets it uses.
- A plain page fetching no widget but its sidebar; on a cold load of a page with widgets, a helper, a diagram and a design, every module asked for by the head, none by a script, each once, and nothing fetched for the diagram; a page loaded in place bringing its preloads.
- Widgets mounting again after a live update and an edited widget or helper reaching open pages, a widget rendered by name inside an unchanged one too.
- A reload revalidating the page and what may change (`304`) and asking for nothing content-addressed.
- A live update finding its page unchanged leaving the DOM untouched, and one that changed it swapping it.
- A live update keeping an unchanged widget's node and state and the scroll, with no frame showing an unmounted placeholder; an edited widget mounting afresh and its old root unmounted.
- Live updates during a load making one more load after it, not one each.
- A link swapping in a page built off-screen, at the top, and back and forward restoring each page's scroll.
- Comments asked for each page source only — never a widget — all at once.
- A comment anchored inside a widget.
- A printed doc showing each widget's first render, terminal lines wrapping on paper, and its diagram on paper with its node's link in the PDF.
- A broken helper's message and an unknown tag's error shown in place, and a project `Callout.tsx` leaving the system `Callout` rendering.
- The sidebar at first paint collapsed with the branch to the page open, its search box drawn, and React taking it over without changing a node, its markup or any box — after a reload too, with the branches the reader opened open from the first frame; React rendering the server's markup; a chevron opening and closing its entry.
- The sidebar staying mounted across links, back and a live update: the same element in the same place in every frame, never empty, highlighting the page the frame shows, the highlight moving in the swap's own commit, the branch navigated to opening and opened branches staying open, the tree edited by a live update reaching it, and the widget never fetched again.
- The sidebar's search box, its searches answered by a stand-in for the endpoint in its answer's shape: typing showing results in the tree's place, grouped by page — the page's row, its sections under it, the more row — the lede of a page found by its title, another document's title over its page, the snippet's words marked; the first row selected the page found by its title, else the best section; arrows moving through every row, round; Escape and the clear button clearing it and bringing the tree back. A section's result swapped in through the page swap, landing on its first matched word with the heading scrolled past — again when scrolled away — its words marked in the section and not in code, a widget, its heading or the next section, the box keeping its text; Escape on the page taking the marks out and leaving the text as it was; opening another section moving them, back taking them out; a doc's chapter loaded whole landing the same way. A page found by its title opening at its top with nothing marked, the page shown and another. `/`, ⌘K and Ctrl+K focusing it, the `/` not typed. A slow answer to earlier text never replacing a later one's results.

### `problem`

*No tests of its own:* a problem is tested where it is found.

### `render`

*24 tests.*

- An image of a design becoming its canvas, and a raw `<img>` found whole with or without its closing slash, its attributes read however they are quoted, spaced or capitalised.
- Headings never taking an id the frame uses.
- Every frame carrying the logo as its tab's icon, before the sidebar's and the outline's title.
- Raw HTML kept and Mermaid fences drawn, one in a `markdown` fence never; a diagram that doesn't parse shown as its error and its source.
- Highlighting without a language label.
- Unknown languages left plain.
- Tables and heading ids.
- A doc's chapters with its outline.
- A deck's slides, never showing their notes.
- Links kept for chapters and slides from other folders, a diagram's links among them.
- A one-file doc's title.
- A page's title.
- A site page's tab title naming the site once when the page is titled like it, whatever the case.
- With comments off, every frame's page script carrying `data-no-comments` and no comment-mode switch; print mode staying print mode.
- The load set in the head: a diagram page coming drawn and preloading nothing for it, its import map naming no pele; a plain page preloading React and its sidebar only, its page script deferred in the head; a widget page preloading its widgets, those they render and their helpers at their addresses, and no unused widget; a design page preloading the design script, the engine and its wasm.
- The sidebar collapsed with the branch to the page shown open, the page's own children included, group headings in place, the empty search box between the title and the tree, no text between its elements, and the tree beside it as JSON with the document file's address; `DevpageSidebar` preloaded. A site with `sidebar: false` having none and preloading nothing.

### `search`

*21 tests.*

- Only what the served document publishes searched: a page no nav lists and a page out of sight never; front matter's `search: true` putting one in, `search: false` taking one out.
- A file that is both a page and a chapter found once, as the chapter; a doc's chapters and a deck's slides at their own addresses, a slide by its notes.
- A page result's title, address, kind, trail, document and lede; a section's heading, path, address, snippet and words.
- A page whose title is the query first, as a title match — the title as typed, in any case, or its start.
- A typo finding a word the index lacks, never one it holds; the last word matched while typed, a word followed by a space whole.
- Every word required, any word when no section holds them all; stopwords not required, a query of them alone finding them; a quoted phrase in a row; a `-word` left out.
- Three sections per page and a count of the rest; ten pages at most.
- A diagram's labels as prose, its source not text; code found by its identifiers whole and by their parts, never in a snippet; a widget's text attributes as prose, its others not.
- Front matter read, never shown, and its keywords searched.
- An empty or blank query finding nothing; an odd one — an unclosed quote, a bare `-`, an emoji, a long one — never failing.
- A frozen index answering as a live one does over the same files, with no writer left, its bytes known, and unchanged by any change it hears.
- Edits, a page dropped from the Topics, a deleted chapter and a new `.gitignore` rule reflected.
- Sections keeping the rendered ids, their text sorted into prose and code; snippets cut at sentences; code split into identifiers and parts; near matches counting a swap as one edit.

### `resolve`

*8 tests, without a socket.*

- A slide's PNG address naming a slide of a deck, or why there is none: a slide the deck doesn't have, a number written otherwise than plainly, a file that isn't a deck.
- A design's PNG taking its theme from the address, and `400` for a bad one.
- A site's page printing as a doc of one chapter.
- Each site's title linking to its own root page.
- A folder with no `.git` answered by what each file is.
- A served folder reached through a symbolic link.
- Nothing outside the served folder answered.
- Every address it answers with carrying the mount prefix.

### `served`

*3 tests.*

- Files and folders inside resolved.
- What climbs out with `..` or links out through a symbolic link refused.
- A deleted file still inside by its path.

### The search's quality

`tests/search_quality.rs`, one test: it starts the real `devpage` on a free
port over `tests/fixtures/search-docs` — a frozen copy of devpage's docs, out
of reach of devpage's own search — and asks its search endpoint 46 queries a
reader would type: a page's title, a word a section is about, a typo, a word
being typed, a code identifier, a quoted phrase, a word left out. Each names
the address it must find first, in the top three pages, or never. It asserts
every one, prints each query's rank, and holds the set's mean reciprocal rank
and success@3 to their floors, `MRR_FLOOR` and `SUCCESS_AT_3_FLOOR` — at the
ranking's result when they were set. A change to ranking that lowers either
fails it; one that raises them raises the floor with it.

### A server's threads

`tests/server_threads.rs`, two tests, each alone in its process so what it
counts is the server's. One starts the real `devpage server` on free ports,
publishes 205 small sites and reads each, and finds the server's thread count
no higher after the last 200 than after the first five — a version holds no
search index until it is searched — and still none higher once one is
searched. With every version building its index as its router is made, it
counted 53 threads before and 1,252 after. The other starts it with
`DEVPAGE_MEMORY_CACHE_VERSIONS=20`, publishes and reads 60 sites, and finds
`devpage_versions_in_memory` on its metrics never above 20, and the first
site, long let go, answering again.

### The restore of a deploy

`tests/deploy_restore.rs`, two `--ignored` tests, one per kind of bucket:
each brings up a server from `deploy/server/compose.yaml` — building the image
from the checkout, with RustFS for S3 and Azurite for Azure Blob — publishes
two versions and starts a thread, deletes the server's volume, starts it on
an empty one, and finds both versions, the thread, the latest at the link and
version 1 unchanged: [the PVC is only a cache](../../viewer/server/constraints/the-pvc-is-only-a-cache.md),
on the example deploy. Without Litestream's restore it fails.

### The server's benchmark

Not a test `cargo test` runs: `.github/workflows/bench.yml`, started by hand,
loads a server to the [envelope](../../viewer/server/measuring-a-server/README.md#the-envelope)
and past it, and reports each target met or missed
([one server holds the envelope](../../viewer/server/constraints/one-server-holds-the-envelope.md)).
The code is `bench/` (`devpage-bench`), a workspace crate never published; the
repository holds no corpus and no results.

- **The stack** is `deploy/server/compose.yaml`'s, the image built from the
  checkout and held to 2 CPUs and 4 GB, with RustFS for S3; the load runs on
  the runner's other cores.
- **The corpus** is generated from a seed, the workflow's input, in memory:
  10,000 documents — half docs, then sites, slides and a few designs, page
  counts and sizes, links, `#heading` links, diagrams and code spread around a
  median with a long tail — and their versions, one to nine a document, five
  on average, 50,000 in all. A document's pages are the same in every
  version; what they say changes. The same seed gives the same corpus.
  devpage's own `docs/` is one more site. It is published through the API,
  eight documents at a time, and that publishing is the first number:
  publishes a second. Then every latest page is read once, so *views*
  measures the served path and *misses* the versions no one has read.
- **The load is open**: each request goes at its scheduled time whether or
  not the last has answered, and its latency counts from that time, so a
  server that stalls is charged for the requests it kept waiting rather
  than sent fewer.
- **The scenarios**, each run three times, the median reported:
  - *views* — readers following a popularity curve over the documents, a few
    read often and most rarely, at 50 a second, then a burst of 500 a second;
    pages, search, a design's PNG and first downloads among them, each
    reported on its own.
  - *misses* — first views of versions no one has read, so each comes from
    the bucket.
  - *writes* — a publish and a comment a second, alongside the views.
  - *herd* — 500 pages open on one document, and a publish to it; each page,
    on hearing of it, fetches the page and the versions again, as a reader's
    page does.
  - *open pages* — 2,000 pages listening throughout.
  - *soak* — the whole envelope for 30 minutes: memory and threads, run once.
  - *knee* — views of each kind of document, from 50 a second and doubling,
    until their p99 crosses 100 ms.
- **The report** is the run's summary page: each number beside its target and
  main's last run, and from `/metrics`, scraped before and after each
  scenario, the stages that took the time. The numbers are the run's
  artifact; a run downloads the last one from `main` to compare against. A
  number has moved only by more than the spread of either run's three; a
  target is missed only by more than it. A baseline at another scale or seed
  is not compared.

The workflow lays `bench/compose.yaml` over the example's compose file: the
server held to 2 CPUs and 4 GB with no swap, its cache sized as on a 10 GB
PVC, its metrics port at 9031. The same binary runs from a laptop against
any server; `--scale` shrinks the corpus, the open pages and the durations,
never the rates:

<Terminal>

```text
$ docker compose -f deploy/server/compose.yaml -f bench/compose.yaml up --build --wait devpage-s3
$ cargo run --release -p devpage-bench -- --server http://127.0.0.1:3031 \
    --metrics http://127.0.0.1:9031/metrics --scale 0.01 --runs 1
```

</Terminal>

`devpage-bench`'s own 13 tests pin the corpus — the same seed, the same
zips; every kind; five versions on average; the same pages in every version;
a zip holding its pages; a folder published whole — the load — a server that
stalls is charged its stall, one that keeps up only its answer; a quantile —
the scrape of `/metrics`, and the report: a target missed only past the
spread, a number moved only past the noise, a run's JSON read back as the
next one's baseline.

### `drive::server`

*31 tests, 37 in Chrome.*

The design tests through the server — a PNG, its `304`, its theme, the watch stream and the canvas page — are listed under [`designs`](#designs), and the Chrome tests under `designs` and `comments`.

- Widgets served compiled, for the document they belong to, each helper imported by its own address; `immutable` at the current version, `no-cache` at any other.
- Every built-in asset `immutable` at its content address, its bare name and a stale hash answering `no-cache`, and the page naming them.
- An asset sent as brotli or gzip as `Accept-Encoding` takes, each with its own ETag, and `304` for a holder of it.
- A page, an image and a document file answering `304` to a strong or weakened ETag until they change; the event stream carrying none.
- A page naming its widgets and carrying their placeholders, code left alone.
- `/` and the site's pages following the entry file's changes without a restart, through an invalid state and back.
- Comment threads over HTTP, against the in-memory store; each write reaching open pages without a watcher.
- A server and the watcher its options start announcing a comment write once.
- A page's event stream carrying each change on its bus, in order.
- A search answering pages as JSON whether comments are on or off.
- Slow pages resolved at once — the server on one async thread of its own — holding up no asset and no cheap page.
- Deployed writes needing the header and stamped with it, over HTTP and in the identity rule alone.
- A thread on a page outside the served folder refused.
- With comments off, every comment request answering `404`, no `.devpage/` created, and the page carrying the setting.
- Nothing outside the served folder served, through HTTP's percent-decoding.
- On a published document, a thread made on the page listed by the API on the page's own path; one started through the API marked on the open page; after a publish that removes its quote, that thread listed under *Detached comments* and the other still marked.

### `drive::server_mode`

*42 tests, and 5 of its configuration; 4 in Chrome.*

- Every `DEVPAGE_*` variable read, the defaults filling what is left out — the cache's limit 80% of the volume, the metrics port 9030, 2,000 versions in memory — and each missing, malformed or foreign variable stopping the server by its name, never echoing its value; another command's variables no mistake.
- A published zip kept in the bucket byte for byte and served at its link — the latest in place at `/d/<id>/`, each version under its own mount — and a version kept unchanged by a later publish, its sha-256 recorded.
- A publish naming its document file with `?entry=`, served from it, and an entry outside the zip refused; a broken link or a zip entry outside the document refusing the publish with `422`.
- An upload over the limit refused with `413`, one over axum's own 2 MB default and under the limit accepted.
- With a user header, a write without it refused with `401` and each version recording its author; without one, every write `anonymous`.
- Documents listed and read with their versions; a document deleted from the store, the bucket and the cache; deleting the latest version making the one before it latest; an id that is no UUID naming no document.
- Concurrent publishes to one document taking one number each.
- With the cache deleted, every version rendering again from the bucket; a restarted server serving what its cache holds without the bucket, and deleting what a run left half-unpacked; no scratch left after a publish.
- Past its limit the cache evicting the least recently read version and a read fetching it again; misses arriving together unpacking a version once; a version bigger than the limit served without being cached, its folder deleted after; a version answering a request never evicted; an evicted version made again under both its mounts. Past its limit in memory, the least recently read versions' routers let go and their files kept, the next read making a router again from disk; a version answering a request keeping its router; a version let go answering its next view as before, its problems with it.
- A published page asking no external address.
- A page's problems — a broken link and a broken `#heading` — found on its first view and shown on every later one without checking again; each version keeping its own.
- The metrics port reporting every stage, a publish and a view counted against the stages they went through, each request by its route and status, and every total; the pages' port answering `/metrics` with `404`; no label naming a document, a version or a page however many are read; a core span counted against its stage, and the text naming only stages.
- A version's search answered from the index its first search builds, under the mount it was asked under — the latest's and the version's own — and the second search answering the same; a version's index built once however many searches arrive, and past the 256 MB budget the least recently searched dropped and built again on its next search; an index bigger than the budget still answering.
- A thread started, replied to and resolved through the API and through the latest's page — the same threads, on the page's unmounted path, oldest first — a page outside the document refused, a second resolve finding nothing.
- With a user header, every comment write — the API's and the page's — needing it and carrying its author.
- Each publish carrying the open threads onto the new version: one kept, one with new context, one whose quote is gone and one whose page is gone detached, each still on the version it was started on; the latest's page listing the detached one as detached; a thread started then on the new version; an earlier version taking no comments; a detached thread staying detached when its words come back.
- Deleting a document deleting its threads.
- The front page saying how to connect, linking to the repository, carrying the logo and listing no documents; `/favicon.ico` being devpage's logo; a page told the version it shows, the latest and every version, version `n` pinned and the latest not.
- A design's download its PNG, painted once and kept beside its version, gone with it; a site, or a missing version, with none; a download counted with its version, seeded so on restart, deleted when the version goes.
- A publish, or deleting the latest version, refreshing pages open at the latest — including one that loaded the new version — and never a version's own.
- In Chrome: a page at the link swapping in each new version, its version menu following; an old version's banner linking to the latest, its menu listing every version, and a publish leaving it unchanged; the banner above a doc's page, never over it, wide and narrow, and a long sidebar's last link above the version bar; a doc's download button saving its PDF, printed once and then read from the cache; every request a published page makes staying under its mount.

### `drive::slide_png`

*5 tests in Chrome.*

- A slide served as a 1920 × 1080 PNG, each slide a different picture, and `404` for a slide the deck doesn't have.
- A slide beside one with a design drawn in under 3 seconds, without waiting for the design; the design's own slide drawn with it painted.
- A slide showing its own PNG answered promptly.
- A repeat request answered from the cache — `304` for a holder of its ETag, the cached picture inside another shot, `508` there for one not drawn — until a change, after which the slide is drawn again.
- Chrome closed when told, and started again for the next shot.

### `drive::validators`

*2 tests.*

- `If-None-Match` compared the weak way, as a list, and `*`.
- Brotli preferred over gzip, and an encoding with `q=0` refused.

### `drive::video`

*19 tests, 2 in Chrome.*

- The silence rules: a pause inside a slide capped at a second and one under
  it kept whole; silence held until what follows is known; a slide's end
  keeping its share of the gap and the next slide starting on speech; leading
  silence dropped; whatever arrived before a slide was asked for left out; a
  fade at every cut.
- The narrator's settings: none set naming the variables, blank counting as
  unset and `OPENAI_API_KEY` never read; OpenAI's address and bearer header
  with the default model; the Azure endpoint selecting Azure over an OpenAI
  key, with its address, `api-key` header and deployment name; an endpoint
  with a path or as a bare host; Azure without its key.
- What the narrator is told: the audience, goal and default tone in its
  instructions and each slide's request with its delivery; the
  making-a-video page showing the template and request exactly as the code
  sends them.
- The default file name; every slide needing notes, the error naming the ones
  without; a deck without `audience` or `goal` refused, naming them; a site
  and a doc refused.

In Chrome:

- A two-slide deck recorded in Chrome for Testing with a synthetic voice fed
  through the same player — no GPT-Live — to an mp4 with a 1920 × 1080 video
  track and a sound track, the deck moving to slide 2 on the way.
- `tests/fixtures/narrated-talk` recorded end to end with GPT-Live on OpenAI,
  when `DEVPAGE_OPENAI_API_KEY` is set. The Azure path has no live test.

### `adapters::watch`

*28 tests.*

Counting the paths the platform's watcher hands the handler, before sifting,
on real folders:

- Writes under an ignored `target/` and under `.git/` never reported.
- A write to a file directly in the served folder (shallow) and in a deep
  folder, a new file directly in the served folder, and a new folder in
  sight and then a file in it, each reported.
- Adding `gen/` to `.gitignore` stopping `gen/`'s events after the re-plan.
- A nested `crate/target/` made after start stopping its events after the
  re-plan, the rest of the crate still reported.
- A shallow folder (`crate/`, holding an ignored `target/`) deleted and made
  again within one burst, and a deep one (`docs/guides/`) deleted and made
  again, each reported again afterwards.
- Writes in the comment store reported, other `.devpage/` folders not.
- An ignored folder a `watch` pattern names (`build/*.svg`) watched, another
  ignored folder not.

The watch set itself (`adapters::watch::set`), on real folders: a folder with nothing
out of sight watched whole; ignored folders and `.git` leaving their parents
shallow and their siblings deep, a nested crate's `target/` included; the
comment store deep and `.devpage` shallow; an ignored folder a `watch`
pattern reaches into watched, the folders under it too (`build/**/*.svg`);
and `**/*.md`, which can match in an ignored folder but reaches into none,
watching none.

Through the listeners:

- A saved page refreshing open pages.
- Ignored files and files outside `watch` not refreshing.
- A comment thread always reaching its page; one the server wrote through the store announced once, by the server, and a hand edit of it once, by the watcher.
- The `watch` list following the entry file.
- A site's `watch` narrowing only its own folder, whichever document devpage was started with.
- Widget files refreshing whatever `watch` says.
- A changed file reaching the design store, and none under `.devpage/`, handed over directly, without waiting on file-system events.

Without waiting on file-system events, fed made-up `notify` events:

- Paths under `target/`, `.git/`, a `.gitignore`d folder and `.devpage/` dropped before any listener hears of them or any is canonicalized.
- A comment thread and a `.gitignore`d file a site's `watch` names still arriving.
- A new `.gitignore` rule taking effect without a restart.
- A burst of saves to one file sending one `change`.

### `widgets`

*28 tests.*

- `<Notes>` taken out of the page, and a project `Notes.tsx` a clash; an unclosed one a problem, so none of it is shown; its `delivery` never shown; a `<Notes>` whose attributes can't be read, or whose delivery isn't a quoted string, a problem and never shown — finding `<Notes>` itself is [`markup`](#markup)'s.
- Widget tags expanded with Markdown content, expression, bare and self-closing props, code left alone — fenced, indented and inline code and HTML comments, but not indented list content.
- An unknown tag replaced by an error, not passed through.
- A `Tab` outside a `Tabs` reported at its page line.
- Nested and inline widgets, an inline widget at the start of a line staying in its paragraph.
- `Example`'s highlighted source.
- TSX compiled with the automatic runtime, widget shims, `__devpageUses` and relative imports rewritten to the addresses given.
- A compile error's line.
- Compiles remembered until the source or an import's address changes.
- A version and compiled bytes made from one reading of the files, whatever is saved in between.
- An edited helper changing its own version and its importers' — through another widget too — and no other widget's; a helper created, edited and removed after its importer was read moving the importer's version; the names a widget can render part of its version.
- Lowercase files as helpers and a system widget's name as a problem.
- A project `DevpageSidebar.tsx` a clash, and a project `Sidebar.tsx` a widget like any other.
- A `Cards` icon drawn on the server and an unknown icon name as a problem.

## The design engine's crates

### `devpage-design`

*140 tests.*

- `parse`, 9: imports, attributes and text with props; JSX's strings without escapes, bare attributes and names with `-`; nesting and numbers kept within their limits, arrays and objects counting as levels and numbers too small named so; JSX whitespace in text; every syntax error, and each problem, at its line and column.
- `ids`, 9: a first parse numbering elements in order; an insert at the front, a removal and an edit keeping every other element's id; keyed siblings keeping theirs when reordered; a matched element's children matched too and new ones numbered, a different root a new one; 100,000 siblings matched in a pass; the edit script a longest common subsequence; a list rewritten past the edit cap — every third row removed, shuffled, reversed, half replaced — keeping the id of every row that survived and giving none another's; gaps nested past the depth cap still giving every row an id of its own. A release bench, ignored, times rewritten lists at 1,000, 10,000 and 100,000 rows.
- `tree`, 3: keys spread evenly sorting in order; a key placed between two — before the first, after the last, in the middle, 2,000 times over — always sorting between them and staying short; children inserted anywhere keeping their order, the same children placed otherwise the same.
- `resolve`, 37: a `key` a string unique among its siblings, two instances of one file their own nodes; props and defaults; an import name unique and never a built-in element's; sizes and spacing never negative; `justify` naming all it takes; `x` and `y` in a frame with auto layout, a negative size, an attribute given twice and an import of something other than a design each a problem; `props` only on the root, and no `x` or `y` on it; `"fill"` only under auto layout; a colour with anything but hex digits in it a problem, not a panic; a tree nested too deep through instances; an instance taking its props and the attributes every element takes; values parsed into their types; opacity kept between 0 and 1; every problem reported at once and each at its place; a missing import a problem at its line, used or not; a design that uses itself; paths kept inside the served folder; an image that doesn't decode — a broken PNG, a truncated JPEG — a problem at its `src`, and a good one's pixels going with the scene; a file larger than 4,096 a side resampled once to fit, by area, a transparent pixel lending no color; `clip` only on a frame; numbers inside a gradient, a `viewBox` and a `d` held to the same limits as any other; an interaction relative to the file that wrote it, one to nowhere a problem, and one only ever to a design; a valid design having none; a text set in the font its `font` names through the `themes.yml` files above it — a lower one adding and overriding names — or in Inter, one design mixing families; a `themes.yml`'s `fonts` a map of lowercase names to paths, anything else a problem at its line, and `font` no key of it; weight picking the face, and a scene carrying only the faces its texts use, once; a weight the font lacks a problem listing those it has, at the `weight` or at the `Text`; an unknown name a problem at the `font`, listing the names there are; every way a font can't be used — missing, outside the served folder, not a font, WOFF, WOFF2, a collection, over 4 MB, unreadable, a folder of no fonts, of two families, of two files of one weight, or holding a bad file — a problem at its `fonts` entry, once; a value given as a prop — `src`, `font`, an interaction — read where it was written, its problem there.
- `resolve::again`, 4: random saves of a design — rows inserted at the front, middle and end, removed, moved, keyed and not, made instances and frames, their text edited, their frame's layout and fill, a component's attributes, a theme's colour — read by rows and through a component's `props` defaults — a line added above everything — each resolved again only as far as it reaches, never whole, the scene a fresh resolve's and the patch bringing the browser's copy there, a problem every problem; an edit to one row's text and a row inserted before the first one operation each, resolving again costing at 100,000 rows what reading the saved file costs and a little more, as at 1,000; a component's save reaching only its instances' roots, a theme's only the nodes that read it, a component's tint from its `props` defaults among them; saves while a design has problems kept, and the fix bringing the last scene across all of them.
- `render`, 53: an edit in every kind of frame — a list's widest row made wider, a list's own fill changed, a child of a frame with no layout resized and one inserted first, a sibling of a `"fill"` child resized, a child of a `"space-between"` frame resized, a fixed-width card of wrapping text in a frame with no layout made wider — visiting the same nodes at 1,000 siblings as at 10,000, none beside what changed, and leaving a drawing laid out and painted as a fresh one, with their numbers at 1,000, 10,000 and 100,000 in an ignored bench; a list's new `align` placing every row again and nothing in them, and every attribute that places a list's children — `align`, `justify`, `padding`, `gap`, its direction — changed from each value to every other on 1,000 rows, each leaving a drawing laid out and painted as a fresh one; a frame with no layout that grows laying out again a child whose size its box reaches; a row of fractional height inserted before the first of 10,000 visiting only the new row, a tile rounding only the rows it shows anew, and the drawing then a fresh one's, box for box and pixel for pixel; layout boundaries and runs — 400 random trees of rows, columns and frames with no layout, every `align` and `justify`, children fixed, hugging, stretching and filling, wrapping text, runs inside runs and boxes placed absolutely, at places in eighths of a pixel — laid out box for box as taffy lays the whole tree out, an edge differing only where taffy's own unrounded edge lies within a thousandth of a pixel of a rounding boundary; one row of a column edited — its text, its height, a row inserted before the first — visiting the same three nodes at 1,000 and 10,000 rows, the damage from the row to the column's end, and the drawing laid out, painted, hit-tested as a fresh one; a row inserted before the first of 1,000 or of 10,000 one insert in the patch and one chunk made, two nodes visited; random saves of a list — inserts at the front, middle and end, removals, moves, resizes, keyed and not — resolved from the saved text and patched onto the canvas's scene, every row the save didn't touch keeping its id and the drawing painting as a fresh one; auto layout, hug, fill, a root that fills on its own hugging, a drawing laid out as the layout lays it out, align and justify, and absolute placement; boxes — a box placed absolutely in a frame with no layout inside auto layout, measured from its own parent frame, and 300 random trees of auto layout and none at fractional places in eighths — rounded box for box as taffy rounds a whole tree; a hugging frame with no layout taking its children's extent, nested ones too; the drawing being the root plus what is painted outside it, cut by clipping, and a drawing 70,000 pixels wide painted whole; a design 100,000 pixels tall streaming its PNG, a tile at its bottom asking the index for a handful of commands; a click on it answered from the index, the topmost interaction, none where a clip hides it; an edit to one row damaging only that row, its tiles as a fresh drawing's; updates — a row growing, a clipping parent resizing, a row added, a child past the top-left, rows removed — painting as a fresh drawing, the area's new strip damaged, a moved corner damaging all; an edit to one row's text visiting only its path — three nodes — on 1,000 rows and on 10,000; an edit applied as a canvas applies it — the patch onto its scene, the drawing by what the patch replaced — costing as much on a balanced design of 10,000 rows as on one of 100; a row that grows moving only the rows after it; random edits of every kind to the checkout design, each applied by its patch as a canvas applies it — attributes, text, sizes, layouts, clips, opacities, children added, removed, moved to the front, the root replaced — each leaving a drawing laid out, painted and holding property-tree nodes as a fresh one; the examples painted tile by tile within 2 of 255 of their references, and a tile the same part of one paint of the whole; no input — hostile values for every attribute of every element, a file cut at every character — making the engine panic; text hugging and centered; fills, gradients along their angle, strokes inside the box, per-corner radius, a shadow following each corner's radius and the spread, sharp and blurred, and its edge the Gaussian at every box size from 100 to 16,000 — fading out with no step, rising 10–90% over 2.563 σ; clip and opacity as one group; vectors scaled into and hugging their view box; images covering their box, PNG and JPEG decoding; images too many for the GPU's atlas a problem at the `src` of the first that doesn't fit, one file shown again taking no more room; a drawing painting the same images as the last painting the last one's blobs, and one whose files, order or shadows differ, or made after none, fresh ones; the example designs painting as their reference PNGs, through tiles; a text laid out in its font, and in a variable one at its weight's point on the axis, which its glyph runs carry; a renderer that drew other fonts laying out as a fresh one, holding one scene's font files.
- `wire`, 10: a patch saying what it changed, by id, and carrying only that; an insert before the first of 100,000 one insert; moves, removes and changes found by id, a row moved out of its parent a new one; a new root replacing the scene; a patch that can't be applied whole — an id the scene hasn't, a sibling under another parent, a node it has already, a move under itself — changing nothing after the operations before it did; the longest run of siblings still in order staying; a scene and a patch carrying both themes; an image file sent once however many nodes show it, a patch carrying only the files the browser lacks, and one whose nodes show a file never sent refused; a font file the same, the browser keeping the one it checked.
- `fonts`, 2: a font file knowing its family, its weight or its weight axis; a folder's italic faces passed over, and one of italic faces only a problem.
- `atlas`, 1: four 4,096-pixel squares filling the atlas at their places, a fifth not fitting.
- `sums`, 2: random inserts, removes and replacements agreeing with a sorted map on every prefix, search and walk; an insert before the first of 100,000 and a prefix at the end in log time.
- `display`, 1: the extent of the chunks' covers knowing its least and greatest edge as they come and go.
- `compare`, 2: the comparison forgiving anti-aliased edges only, and catching a missing divider or icon on the checkout design.
- `bands`, 4: the band writer holding one row of tiles at a time and writing their rows in order, a tile it can't have stopping it; a band holding as many rows as fit in 16 MB, a whole row of tiles up to 16,384 pixels wide, at least one row however wide; a drawing 1,000,000 pixels wide written in strips holding at most 16 MB at once, its pixels right at both ends and across a strip's seam and a band's; a strip painting as the part of the tile it replaces.
- `tiles`, 3: a view needing the tiles it crosses, its middle first, and a tile it doesn't show landing nowhere; the tile cache holding its cap, the least recently used dropped first; damage making exactly the tiles it overlaps stale at every level, a stale tile still there until painted again.
- The crate, 1: the core naming no adapter and no rasterizer.
- `shadow`, 1: a shadow in pieces matching it as one image, with no seam and within the bound for how wide its blur is on the screen, at fractional scales, for every kind of piece, a box thinner than its blur, and a dozen random outlines.

### `devpage-design-wasm`

*9 tests.*

- A scene sized, painted and hit-tested.
- A design in a font file, by its scene and by a patch bringing another weight, laid out and painted as the server paints its PNG, pixel for pixel.
- The engine's atlas model and vello's own resolver, as the module ships it, agreeing on 4,000 random sets of images: whether they fit, the first left out, and each one's place.
- The two agreeing on a real drawing's image files and shadows, replayed through the one paint entry point as a tile is, and on the same drawing updated to too many files.
- A patch before any scene, or garbage, doing nothing.
- Problems keeping the scene until a patch clears them.
- A theme switch painting the scene again in the other theme.
- A click landing only on what a clip shows.
- A whole scene damaging every tile of both themes, a patch only where the node it changed was and is, in each theme drawn, and a theme switched back to keeping its drawing.

### `devpage-design-module`

*2 tests.*

- The vendored module built from the sources as they are.
- The fingerprint changing with an engine source, the `wasm` profile and a locked version in the module's tree, but not with a package outside it.

## What has no test

<Callout kind="note" title="Not covered by any test">

syntax highlighting except through the `render` tests; a canvas reopening
its stream after devpage restarts, closing streams in a hidden tab, and
drag-to-pan and pinch on the canvas; chrome-headless-shell's and Chrome for
Testing's first download (it needs an empty cache folder); GPT-Live on Azure;
`devpage video` stopping on a signal; fullscreen on a deck (headless Chrome refuses it without a real user
gesture); and how the light and dark themes look, which is checked by eye.

</Callout>
