# Tests

What devpage's test suite covers, module by module, how the tests are written,
and what has no test. It is a reference page under
[Developing devpage](../developing-devpage/README.md), for someone about to
change a module or wondering whether a behaviour is pinned.

<Terminal>

```text
$ cargo test                  # 334 tests in the five crates; needs no network and no Chrome
$ cargo test -- --ignored     # the 80 tests that drive Chrome or call GPT-Live
```

</Terminal>

## How they are written

`cargo test` runs every crate in the workspace: 91 in `devpage`, 159 in its
core `devpage-core`, 75 in the engine `devpage-design`, 7 in
`devpage-design-wasm`, whose tests run natively, and 2 in
`devpage-design-module`. Tests are written as `#[cfg(test)]` modules beside
the code they test — the canvas's Chrome tests in
`src/drive/server/canvas_tests.rs` and the comments' in
`src/drive/server/comments_browser.rs`, child modules of `server`. The `tests/`
folder holds one test, [the search's quality](#the-searchs-quality), and
fixtures — `tests/fixtures/sync-design`, a doc of three
chapters that the doc and PDF tests load, `tests/fixtures/narrated-talk`,
a deck of three slides with notes that the live video test records, and
`tests/fixtures/search-docs`, a frozen copy of devpage's docs that the
search's quality is measured on. Route tests start the real router on
a free port on `127.0.0.1` and talk to it with `reqwest`. The core's
external-link tests ask a scripted table through the checker's `Ask` port,
and the HTTP adapter's point it at a local web server, so the suite needs no
network.

Of the 80 tests marked `#[ignore]`, 79 drive Chrome, and one of them also
calls GPT-Live; the other asks Google's server for the pinned Chrome's zips,
so it needs the network.
All but the two `video` tests drive chrome-headless-shell through
chromiumoxide — the copy
[`devpage pdf` downloads](../../viewer/pdf/README.md#the-chrome-it-drives)
into devpage's cache folder, fetched by the first test that needs it — never
a Chrome installed on the machine, so a run puts nothing in the macOS Dock.
They run in parallel. Run them with `--ignored` after changing the page
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

Nineteen tests save a file and wait for devpage to hear of it through the
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
- two `drive::server` tests —
  `the_entry_document_follows_its_file_without_a_restart` and
  `a_server_and_the_watcher_its_options_start_announce_a_write_once`;
- two Chrome tests in `drive::server::comments_browser` —
  `selected_text_becomes_a_thread_in_the_margin_and_in_the_store` and
  `a_live_update_with_the_comments_shown_swaps_the_page_in_place`.

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

- A subscriber hearing its own scope and every-document changes, never another mount's.
- One that falls behind hearing a refresh rather than losing an update.
- `heard` reporting any change in scope since the last look, without waiting.
- `next` waiting for its scope and ending when the bus is gone.

### `adapters::http`

*2 tests, against a local web server.*

- `devpage check` asking each external address once and reporting every link to it.
- The preview checking in the background and refreshing open pages.

### `changes`

*1 test.*

- The page reading the same `change` and `comments /<page>` messages as before the bus was typed.

### `check`

*15 tests.*

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
- The fixture doc, the devpage talk and devpage's own docs free of broken internal links.

### `comments`

*8 tests — 4 of the port, 4 of its YAML adapter; 6 in Chrome, in `drive::server::comments_browser`.*

- A page that climbs out of the served folder and a made-up id refused.
- Every write announcing its page once, and a failed one nothing.
- The local author without a `.git` folder.
- Timestamps.
- A thread's whole life leaving `git status` clean.
- A thread file exactly the YAML it always was.
- A thread file that doesn't parse named and left alone.
- Only a change the store didn't make itself left for the watcher to announce.

In Chrome:

- Commenting off until comment mode is on, a comment made, the switch remembered after a reload.
- Selected text becoming a thread in the margin and the store, then detaching.
- A paragraph comment on a nested list anchoring again after a reload.
- The comment icon and a wide table staying inside 390 px and 820 px windows.
- **C** on a deck.
- A live update with the comments shown swapping the page in place rather than reloading it.

### `designs`

*10 tests in the store; 14 more through the server, and 31 in Chrome, in `drive::server`.*

- The store announcing only the designs made from a changed file, and resolving a design again when a missing import appears; a save landing while a design resolves never leaving a stale scene kept.
- An image that doesn't decode a problem at its `src` in what `devpage check` reads and in the PNG.
- A drawing too big to paint a problem in the store and in what `devpage check` reads; images too many for the GPU's atlas a problem at the `src` of the first that doesn't fit.
- An image file decoded once per read of it: a save of the design showing it doesn't decode it again, a save of the file does.
- A real file named like a design's PNG served as it is; a link to a design's PNG checked as the design.
- No design file — a pasted zero-width space in a colour, deep nesting, a huge number, a truncated file — stopping the store, which still sees saves afterwards.
- Nothing outside the served folder read.
- A design's PNG answering `304` from its scene without painting, until a design it uses changes.
- A PNG, or `422` and the problems, and `404` for a design outside the served folder or missing.
- A design as the entry document; the canvas page titled without `.design`.
- The design service answering only `Watch`.
- The watch stream over gRPC-Web sending the scene, a patch of only what changed, nothing for a no-op, problems, and a deleted design coming back when written again.
- One stream over several designs sending each scene once, patches naming their design, going on after one is deleted, and bringing it back when it is written again.
- `devpage check` naming a broken embedded design's `file:line:col`.

In Chrome:

- Embeds inside widgets on a page in a subfolder — in a `Callout`, a `Tab`, a `Steps` item and a `Callout` inside a `Steps` item — each finding its design beside the page.
- The canvas on the GPU and on the CPU each matching the PNG by the engine's comparison, over the whole drawing and a margin of plain page around it.
- On the GPU, as in the PNG: a file wider than the GPU's atlas, twelve distinct 2000-pixel files together, and a save swapping six of them for new ones; shadows drawn in pieces — square, sampled and disc corners, a pill, a thin box, ellipses in wedges.
- A navigation from a design whose four 4,096-pixel images fill the GPU's atlas to one with an image of its own drawing the second as its PNG, on a canvas page and inside an embed.
- A click navigating, back returning, an overlay opening and closing; an interaction inside an instance from another folder opening its own target.
- The wheel zooming about the pointer and panning.
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

*6 tests.*

- Links, dropdowns, icons and icon-only links rendered in order.
- The link holding the page shown highlighted, a dropdown holding it too.
- Missing files and unknown icons as problems.
- No navbar rendering nothing, the comment-mode switch in the corner, and the theme selector off with `theme-selector: false` while the switch stays; with comments off, no switch.
- A logo before the title and as the tab's icon, for a doc and a deck too.
- A missing logo as a problem and the page shown without it.

### `nav`

*4 tests.*

- Links and `###` groups read from the Topics section only.
- The tree stopping at a cycle.
- A Topics entry outside the site or to a missing file is a problem.

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

*20 tests.*

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

### `drive::server`

*30 tests, 37 in Chrome.*

The design tests through the server — a PNG, its `304`, its theme, the watch stream and the canvas page — are listed under [`designs`](#designs), and the Chrome tests under `designs` and `comments`.

- Widgets served compiled, for the document they belong to, each helper imported by its own address; `immutable` at the current version, `no-cache` at any other.
- Every built-in asset `immutable` at its content address, its bare name and a stale hash answering `no-cache`, and the page naming them.
- An asset sent as brotli or gzip as `Accept-Encoding` takes, each with its own ETag, and `304` for a holder of it.
- A page, an image and a document file answering `304` to a strong or weakened ETag until they change; the event stream carrying none.
- A page naming its widgets and carrying their placeholders, code left alone.
- `/` and the site's pages following the entry file's changes without a restart, through an invalid state and back.
- Comment threads over HTTP, against the in-memory store; each write reaching open pages without a watcher.
- A server and the watcher its options start announcing a comment write once.
- A page's event stream carrying only its documents' changes.
- A search answering pages as JSON whether comments are on or off.
- Deployed writes needing the header and stamped with it, over HTTP and in the identity rule alone.
- A thread on a page outside the served folder refused.
- With comments off, every comment request answering `404`, no `.devpage/` created, and the page carrying the setting.
- Nothing outside the served folder served, through HTTP's percent-decoding.

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

*75 tests.*

- `parse`, 9: imports, attributes and text with props; JSX's strings without escapes, bare attributes and names with `-`; nesting and numbers kept within their limits, arrays and objects counting as levels and numbers too small named so; JSX whitespace in text; every syntax error, and each problem, at its line and column.
- `resolve`, 30: props and defaults; an import name unique and never a built-in element's; sizes and spacing never negative; `justify` naming all it takes; `x` and `y` in a frame with auto layout, a negative size, an attribute given twice and an import of something other than a design each a problem; `props` only on the root, and no `x` or `y` on it; `"fill"` only under auto layout; a colour with anything but hex digits in it a problem, not a panic; a tree nested too deep through instances; an instance taking its props and the attributes every element takes; values parsed into their types; opacity kept between 0 and 1; every problem reported at once and each at its place; a missing import a problem at its line, used or not; a design that uses itself; paths kept inside the served folder; an image that doesn't decode — a broken PNG, a truncated JPEG — a problem at its `src`, and a good one's pixels going with the scene; a file larger than 4,096 a side resampled once to fit, by area, a transparent pixel lending no color; `clip` only on a frame; numbers inside a gradient, a `viewBox` and a `d` held to the same limits as any other; an interaction relative to the file that wrote it, one to nowhere a problem, and one only ever to a design; a valid design having none.
- `render`, 31: auto layout, hug, fill, a root that fills on its own hugging, a drawing laid out once, align and justify, and absolute placement; a hugging frame with no layout taking its children's extent, nested ones too; the drawing being the root plus what is painted outside it, cut by clipping, and a drawing too big being a problem; no input — hostile values for every attribute of every element, a file cut at every character — making the engine panic; text hugging and centered; fills, gradients along their angle, strokes inside the box, per-corner radius, a shadow following each corner's radius and the spread, sharp and blurred, and its edge the Gaussian at every box size from 100 to 16,000 — fading out with no step, rising 10–90% over 2.563 σ; clip and opacity as one group; vectors scaled into and hugging their view box; images covering their box, PNG and JPEG decoding; images too many for the GPU's atlas a problem at the `src` of the first that doesn't fit, one file shown again taking no more room; a drawing painting the same images as the last painting the last one's blobs, and one whose files, order or shadows differ, or made after none, fresh ones; the example designs painting as their reference PNGs; the comparison forgiving anti-aliased edges only, and catching a missing divider or icon on the checkout design.
- `wire`, 3: a patch carrying only the nodes that changed; an image file sent once however many nodes show it, a patch carrying only the files the browser lacks, and one whose nodes show a file never sent refused.
- `atlas`, 1: four 4,096-pixel squares filling the atlas at their places, a fifth not fitting.
- `shadow`, 1: a shadow in pieces matching it as one image, with no seam and within the bound for how wide its blur is on the screen, at fractional scales, for every kind of piece, a box thinner than its blur, and a dozen random outlines.

### `devpage-design-wasm`

*7 tests.*

- A scene sized, painted and hit-tested.
- The engine's atlas model and vello's own resolver, as the module ships it, agreeing on 4,000 random sets of images: whether they fit, the first left out, and each one's place.
- The two agreeing on a real drawing's image files and shadows, replayed as the canvas replays it, and on the same drawing with too many files.
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
drag-to-pan and pinch on the canvas; chrome-headless-shell's and Chrome for
Testing's first download (it needs an empty cache folder); GPT-Live on Azure;
`devpage video` stopping on a signal; fullscreen on a deck (headless Chrome refuses it without a real user
gesture); and how the light and dark themes look, which is checked by eye.

</Callout>
