# Tests

What devpage's test suite covers, module by module, how the tests are written,
and what has no test. It is a reference page under
[Developing devpage](../developing-devpage/README.md), for someone about to
change a module or wondering whether a behaviour is pinned.

<Terminal>

```text
$ cargo test                  # 566 tests in the five crates; needs no network and no Chrome
$ cargo test -- --ignored     # the 118 tests that drive Chrome or Docker, call GPT-Live, or time the engine
```

</Terminal>

## How they are written

`cargo test` runs every crate in the workspace: 163 in `devpage`, one of
them in `tests/`, 237 in its
core `devpage-core`, 154 in the engine `devpage-design`, 10 in
`devpage-design-wasm`, whose tests run natively, and 2 in
`devpage-design-module`. Tests are written as `#[cfg(test)]` modules beside
the code they test, except the tests that drive Chrome: each Chrome suite is
a `#[cfg(test)]` module of `src/drive/browser_tests/` — `page_script`,
`canvas`, `comments`, `pdf`, `slide_png`, `video`, `server_mode`,
`server_mode_pages` and `server_mode_comments` — on `chrome::testing`, which
launches one Chrome for the whole run and gives each test a browser of its
own inside it — a browser context, with its own storage, cache and
connections — serves the test's router on loopback and waits on its pages.
A test waits on the condition it is after — a state of the DOM or the page
script with `until` and `until_is`, a request answered with
`until_answered`, frames drawn with `next_frames`, the promise the page
script's own `load` returns, a screen that shows the server's PNG — never a
fixed time; where it checks that something does not happen, it first waits
for a sign that it would have by then, such as a later request answered or a
later key's effect. What a page's server produces — its HTML, a route's
answer, a print call — is tested without Chrome, beside that code; a Chrome
test is kept for what only a browser does: layout, the page script, painting,
input and live updates.
Below, each suite's tests are listed under the module whose behaviour they
pin. The `tests/`
folder holds two, [the search's quality](#the-searchs-quality) and
[the restore of a deploy](#the-restore-of-a-deploy), and fixtures — `tests/fixtures/sync-design`, a doc of three
chapters that the doc and PDF tests load, `tests/fixtures/narrated-talk`,
a deck of three slides with notes that the live video test records, and
`tests/fixtures/search-docs`, a frozen copy of devpage's docs that the
search's quality is measured on. Route tests start the real router on
a free port on `127.0.0.1` and talk to it with `reqwest`. The core's
external-link tests ask a scripted table through the checker's `Ask` port,
and `adapters::external`'s point it at a local web server, so the suite needs no
network.

Of the 118 tests marked `#[ignore]`, 109 drive Chrome, and one of them also
calls GPT-Live; two run the server deploy example in Docker, one at a time,
since they share its compose project; six time the engine in a release build —
a save to its patch in the store at 1,000, 10,000 and 100,000 rows, what an
edit to one row of a column costs a drawing at those lengths, an edit in every
kind of frame at those lengths, how an edit's update grows to 200,000
siblings, one update and its undo over and over for a profiler, and matching
rewritten lists; the last asks
Google's server for the pinned Chrome's zips, all at once, so it needs the
network.
All but the three `video` tests drive chrome-headless-shell through
chromiumoxide, launched by `chrome::testing` in the shot profile — the copy
[`devpage pdf` downloads](../../viewer/pdf/README.md#the-chrome-it-drives)
into devpage's cache folder, fetched by the first test that needs it — never
a Chrome installed on the machine, so a run puts nothing in the macOS Dock.
They run in parallel, so the longest bounds the run: the `video` tests,
which record a deck as long as its slides play, and start last in the
run's order — [how long they take](#how-long-the-chrome-suites-take). Run
them with
`--ignored` after changing the page
script, the design script, the module, the stylesheet, a widget, print mode,
`devpage pdf` or a slide's PNG. The three `video` tests drive Chrome for
Testing instead — the copy [`devpage video` downloads](../../viewer/making-a-video/README.md#the-chrome-it-drives),
fetched the same way; run them after changing `devpage video` or its
recorder, `assets/video.js`. The live one records
`tests/fixtures/narrated-talk` with GPT-Live, which bills per minute, and
passes without doing anything unless `DEVPAGE_OPENAI_API_KEY` is set. The shot profile starts the shell
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
- two `drive::http` tests —
  `the_entry_document_follows_its_file_without_a_restart` and
  `a_server_and_the_watcher_its_app_starts_announce_a_write_once`;
- two Chrome tests in `drive::browser_tests::comments` —
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

### How long the Chrome suites take

`cargo test --bin devpage -- --ignored` — every Chrome suite, the GPT-Live
test passing at once without a key — on an Apple M5 Max with 18 cores, the
binary already built: before the suites shared one Chrome and waited on
conditions, after, and since the `video` tests record with a short
[pacing](../../viewer/making-a-video/README.md#how-it-is-recorded). One at a
time, the same 102 tests; the runs after hold more — five canvas tests that
landed meanwhile, the shared Chrome's own, and by the last column 110 in all.

| | Before | After | Short pacing |
| --- | --- | --- | --- |
| The run, in parallel | 22.4–23.5 s | 19.4–20.1 s | 10.4–11.0 s |
| The same tests, one at a time | 145 s | 76 s | |
| `page_script`, one at a time | 27.3 s | 4.7 s | |
| `canvas`, one at a time | 75.9 s | 35.7 s | |

The slowest tests, one at a time:

| Test | Before | After | Short pacing |
| --- | --- | --- | --- |
| `video::a_recorded_slide_showing_a_slides_png_gets_it_drawn` | 18.9 s | 13.1 s | 5.1 s |
| `video::a_deck_is_recorded_to_a_1920_by_1080_mp4_with_its_voice` | 13.4 s | 13.1 s | 4.3 s |
| `canvas::images_larger_than_the_gpu_atlas_draw_on_the_gpu_as_in_the_png` | 6.0 s | 3.4 s |
| `canvas::the_canvas_on_the_gpu_matches_the_png` | 5.4 s | 0.95 s |
| `canvas::the_canvas_on_the_cpu_matches_the_png` | 5.4 s | 0.7 s |
| `canvas::a_pan_costs_the_same_on_a_huge_design_as_on_a_small_one_on_the_gpu` | 5.1 s | 4.9 s |
| `canvas::a_cpu_canvas_paints_when_its_scene_arrives_before_the_worker_has_loaded` | 4.3 s | 0.3 s |
| `canvas::a_deleted_design_comes_back_when_written_again` | 3.8 s | 0.25 s |
| `page_script::widgets_mount_on_load_and_after_a_live_update` | 3.3 s | 0.1 s |

What is left is what the tests are about. A recorded deck plays in real
time, but the tests give the recorder a short pacing — their stand-in voice
is buffered a tenth of a second before a slide plays and is over after a
fifth of a second of quiet, where `devpage video`'s default is two seconds
and one and a half — so what remains is Chrome for Testing starting, the
recorder arming and each slide's speech and lead. A pan is timed over 60
frames on each of two designs. In parallel the run is still ended by the
`video` tests, which the test runner, taking tests in the order of their
names, starts last: without them the run takes 7.8 s. One at a time, the
rest of the run — less the test that asks Google's server for the pinned
Chrome — takes about 50 s.

## By module

`devpage-core`'s modules by their names; the binary's by their paths under
`adapters::` (what devpage's core is wired to) and `drive::` (what drives it).

### `addresses`

- Every built-in asset at its content address, the hash of its bytes; its bare name and a stale hash finding it, not as current.
- An asset naming the others by their content addresses.
- A widget's address carrying the mount and the version it is given.

### `answer`

*Without a socket.*

- A widget's module answered compiled for the document whose widgets folder holds it, each helper imported by its own address; immutable at the address its page names it by, and at any other the current module, never immutable; an import without an extension finding the `.ts`; a system widget the same, an unknown one not found; nothing outside the served folder.
- A widget whose helper — reached through another helper — doesn't compile answered by a module that throws, naming that helper.
- A design opening on its canvas page, in the theme its address names, and passing any other over.
- An edited `.gitignore`d doc served fresh: no document is kept between requests.
- A slide's PNG address naming a slide of a deck, or why there is none: a slide the deck doesn't have, a number written otherwise than plainly, a file that isn't a deck.
- A design's PNG taking its theme from the address, and `400` for a bad one.
- A site's page printing as a doc of one chapter.
- Each site's title linking to its own root page.
- A folder with no `.git` answered by what each file is.
- A served folder reached through a symbolic link.
- Nothing outside the served folder answered.
- Every address it answers with carrying the mount prefix.

### `adapters::bucket`

- A `DEVPAGE_BUCKET_URL` that is no URL, or names no bucket devpage reads, refused by the variable's name, never echoing its value.

### `adapters::bundle`

- The zip holding exactly what the check follows — the document, its pages, linked images, a design and its import, the logo, the project widgets and their imports — and not a draft or a secret in the same folder; a link beside the document's folder widening the zip to hold it.
- What is packed unpacking file for file.

### `adapters::changes`

- Every subscriber hearing every change, in order.
- One that falls behind hearing a refresh rather than losing an update.
- `refreshed` reporting a refresh since the last look, without waiting — a page's comments not counting, falling behind counting.
- `next` waiting for a change and ending when the bus is gone.

### `adapters::chrome`

- Chrome's sandbox on unless `DEVPAGE_CHROME_SANDBOX` is `off`, in either profile, and any other setting refused.
- The shot profile drawing with the GPU, and the recording one capturing its own tab, playing audio without a gesture and hiding scrollbars.
- The pinned Chrome and Chrome for Testing already in a cache folder used as they are, with no network call.
- A failed download naming the browser, its version, the platform and the address.

In Chrome:

- A tab opened once its page is ready, sending the shot header only when opened to be shot, and evaluated in, a promise awaited.
- Two tests' browsers one Chrome, each with storage of its own.

On the network:

- Both browsers' zips at the pinned version answering `200` for every platform.

### `adapters::external`

*Against a local web server.*

- `devpage check` asking each external address once and reporting every link to it.
- The preview checking in the background and refreshing open pages.

### `adapters::git`

- The user name without a `.git` folder.

### `adapters::sqlite`

*Its threads are tested through `drive::server_mode`.*

- A version number given once: a second publish that read the same next number refused, and a deleted version's number never given again.
- Deleting the latest version making the one before it latest, and deleting the only one deleting the document.
- Deleting a document deleting its versions and nothing else.

### `cache`

- The least recently read versions going, oldest first, until the cache is at 90% of its limit; nothing going under it.

### `changes`

- The page reading the same `change` and `comments /<page>` messages as before the bus was typed.

### `check`

- `<Notes>` on a page that isn't a slide, a second one on a slide, and one without its `</Notes>`, as problems — in `devpage check`, however a slide is reached, and in the preview.
- A site's navbar reported once and its links followed.
- A missing logo reported once.
- Widget problems — an unknown widget tag, a project widget with a system widget's name, a widget or a helper that doesn't compile, a widgets folder's problems reported once however many pages.
- Missing files and headings and links climbing out of the served folder.
- A diagram that doesn't parse reported at its fence's line with pele's message, past front matter, in `devpage check` and in the preview; one in a `markdown` fence never.
- A slide number the deck doesn't have.
- A heading in a doc checked against the ids the doc renders.
- A broken anchor into another page — a Markdown file and a doc — found alike by `devpage check` and the preview.
- Topics checked against the page's own site, and a page in no site checked as a doc.
- A broken linked document named by its address, not an absolute path.
- The preview reporting each problem once to its reporter.
- A design checked and its files followed through the design store the check is given, resolved there once; a page's problems naming an embedded design as the server's store holds it, a save the store is told of clearing them.
- The fixture doc, the devpage talk and devpage's own docs free of broken internal links.
- What a deck is drawn from: its file, its slides, what they embed — a design's PNG by its design, an image not there yet, a path from the top, another deck's slide PNG and that deck in turn, once however they embed each other — and never a page a slide only links to or an address on the web.

### `comments`

*Of `Comments`, of its YAML adapter and of re-anchoring; in Chrome, in `drive::browser_tests::comments`, and on a published document, in `drive::browser_tests::server_mode_comments`.*

- A page that climbs out of the served folder and a made-up id refused.
- Every write announcing its page once, keyed `/a.md` however it was named, and a failed one nothing.
- A thread started only on a file in the served folder: a lone page no document links to accepted; a missing page, a folder, an empty page and one that climbs out refused.
- A reply and a resolve going by the thread's id alone, a detached thread included.
- Carry reading a page as it is shown: front matter left out, a widget tag expanded; writing nothing itself, and only the changed page announced once it is committed.
- A header author trimmed, and a missing or blank one refused.
- Timestamps.
- A thread's whole life, found by its id alone, leaving `git status` clean.
- A thread file reached through a symbolic link out of the served folder never found.
- A thread file exactly the YAML it always was.
- A thread file that doesn't parse named and left alone.
- Only a change the store didn't make itself heard, for its page; the page itself and another file beside the threads never.
- A page's text read as the page script reads it — tags gone, entities decoded, white space squeezed, scripts and styles left out; a quote with its context keeping its anchor, one whose context changed taking its new context, counted in characters, and one that is gone detached.

In Chrome:

- Commenting off until comment mode is on, a comment made, the switch remembered after a reload.
- Selected text becoming a thread in the margin and the store, then detaching.
- A paragraph comment on a nested list anchoring again after a reload.
- The comment icon and a wide table staying inside 390 px and 820 px windows.
- **C** on a deck.
- A live update with the comments shown swapping the page in place rather than reloading it.

### `designs`

*In the store; through the server, in `drive::http`; and in Chrome, in `drive::browser_tests::canvas`.*

- Saves through the store — rows inserted at the front and the end, edited and removed, a component and a theme changed, a break and its fix — each sent as the store's one patch, which brings the browser's copy to the store's scene, nothing resolved whole after the first resolve, and a stream joining late sent the scene whole.
- The store announcing only the designs made from a changed file, and resolving a design again when a missing import appears; a save landing while a design resolves never leaving a stale scene kept.
- A change to a font file, or to any file in a font folder, resolving again only the design set in it; a folder with two regular faces a problem at its `fonts` entry in the `themes.yml`, in the store and in a fresh one, as `devpage check` makes.
- An image that doesn't decode a problem at its `src` in what `devpage check` reports and in the PNG.
- A design 100,000 pixels tall drawable in the store, with no problem for `devpage check`, its PNG streamed band by band from the store's tile cache, which never holds more than its 64 MB; a PNG asked for again painting no tile, and after an edit to one row only the tiles over it — undone, those again, and the PNG as before; each theme of a themed design keeping its own tiles, and a design that stops using its themes dropping the dark ones; a writer that fails stopping the painting within the first band; a save landing while a PNG's reader has stopped reading, the PNG then ending rather than finishing from the next scene; a design too wide for a row of tiles in one band banded in strips, its PNG right and nothing cached; images too many for the GPU's atlas a problem at the `src` of the first that doesn't fit.
- An image growing mid-session past what the GPU's atlas holds the design's problem, at its `src`, in place of a PNG, a save meanwhile kept; once mended, the PNG the one a fresh store paints.
- A save that changes a design applying its edits once, to the design's one scene, which the store's scene and PNG then show; a save that changes nothing applying none.
- An image file decoded once per read of it: a save of the design showing it doesn't decode it again, a save of the file does — the scene keeping the decode it has while the bytes are the same.
- A real file named like a design's PNG served as it is; a link to a design's PNG checked as the design.
- No design file — a pasted zero-width space in a colour, deep nesting, a huge number, a truncated file — stopping the store, which still sees saves afterwards.
- Nothing outside the served folder read.
- The store knowing everything a design is made of — itself, its import, its themes, its font folder and its font files, from one resolve, whether or not it resolves — a file it asked for that isn't there named as it would be, a `themes.yml` above it or the design's own missing file.
- An image a design shows added after it: the design announced and drawn with it.
- A font added to a design's font folder: the design announced and drawn with it, and announced again when the font is removed.
- Two folders in one store — two versions of a document — sharing its one tile cache and nothing else: the same address in each its own design with its own validator, the same mount and folder the design already kept, and a folder no one holds leaving the store with its tiles.
- A design's PNG answering `304` from its scene without painting, until a design it uses changes.
- A PNG, or `422` and the problems, and `404` for a design outside the served folder or missing.
- A design as the entry document.
- The design service answering only `Watch`.
- The bytes the watch stream sends for a move, an insert and a new image file fixed, byte for byte, whatever the engine's edits and the wire's encoding of them are cut into.
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
- A canvas without WebGPU painting when its scene reaches the worker before the worker has loaded the engine; asking the worker for the tile under the view's middle first, and never more than the plan's budget in flight at once.
- An undone typo clearing the problems.
- An image growing mid-session past what the GPU's atlas holds putting the problem on the problems bar, a save meanwhile kept, and once mended the canvas, on the GPU, matching the PNG.
- A save changing only a fill, on the GPU, repainting its tiles with their images, as in the PNG — a tile beside them that paints no image leaving the canvas's atlas as it was.
- A design broken mid-session — an image that stops decoding, or one that grows past what the GPU's atlas holds — keeping its problem on the bar, a save meanwhile not shown, and a zoom into a part never painted painting it from the last good drawing, on the CPU and on the GPU.
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

- A flowchart drawn to SVG with its `click` links, its tooltip as their title, an `https://` target kept through the engine's `URL`, and its colors left as `--pele-*` variables.
- pele's error as the diagram's — a parse error, no diagram type — and the engine drawing the next one.
- A diagram drawn before, or one pele rejected, coming from the cache; only new ones sent to the thread.
- A diagram past the time limit failing with how long it may take, and not kept, so it is tried again.
- A diagram past the memory limit failing as out of memory, and the next one drawn in a fresh runtime.
- Fences in rendered HTML replaced by their drawings or by the error block with the source, escaped; other code left alone.

### `document`

- Each kind parsed.
- A deck's audience, goal, tone and pronounce, each slide's notes and delivery; those fields refused on the other kinds, and `context` refused.
- A navbar read in order with its icons and dropdowns, `sidebar` and `theme-selector`, and its bad shapes refused.
- Bad kinds, fields and missing files rejected.
- A Markdown file as a one-chapter doc.
- The served folder as boundary and address root.
- A document outside it refused.
- A site's `watch` list.
- Only YAML with a devpage `kind` is a document file.
- What a kind downloads as: a doc or slides a PDF, a design a PNG, a site nothing.
- The download name — what `devpage pdf` and `devpage video` write, and what a version downloads as — the title made safe, with its extension.

### `external`

*Against a scripted `Ask`.*

- `devpage check` asking an unreachable address once more before reporting it, and a broken one once.
- Outside a runtime, the preview asking nothing.
- The preview showing an unreachable address only after three failures in a row, and clearing it when it answers.

### `folder`

*On real folders, without a watcher.*

The served folder, `ServedFolder`:

- Files and folders inside resolved.
- What climbs out with `..` or links out through a symbolic link refused.
- A deleted file still inside by its path.
- A mounted folder addressing its files under the mount, its names the same anywhere.

The document index and owners:

- Document files found outside what `.gitignore` excludes.
- A page owned by the nearest site above it; an owner read as its file says now, and an invalid site still the owner.
- Nested `.gitignore` files and `.git` folders out of sight.
- Changes keeping it current, and saying when the folders to watch may differ.
- A widgets folder belonging to the document that names it.

What a changed path means, ported from the watcher so the rules are pinned on every platform:

- Paths under `target/`, `.git/`, a `.gitignore`d folder and `.devpage/` never mattering; a comment thread and a `.gitignore`d file a site's `watch` names still mattering.
- A new `.gitignore` rule taking effect without a restart.
- Ignored files and files outside `watch` not refreshing.
- The `watch` list following the entry file; an invalid entry file keeping its last valid document, judging by its `watch` list and saying why.
- A site's `watch` narrowing only its own folder, whichever document devpage was started with; the entry's `watch` not quieting another folder.
- Widget files refreshing whatever `watch` says.

The watch set (`sight`): a folder with nothing out of sight watched whole;
ignored folders and `.git` leaving their parents shallow and their siblings
deep, a nested crate's `target/` included; the comment store deep and
`.devpage` shallow; an ignored folder a `watch` pattern reaches into watched,
the folders under it too (`build/**/*.svg`); and `**/*.md`, which can match in
an ignored folder but reaches into none, watching none.

### `html`

*On strings.*

- Text escaped for an element's text and a quoted attribute, and back.
- A tag's attributes read however they are quoted, spaced or capitalised.
- Every value of one attribute found in tags only — never in a comment, escaped code or text, or under another name such as `xlink:href`.
- An address on the web, with another scheme, or a file with its fragment — a colon in a path or a fragment being no scheme.

### `icons`

- Lucide icons and GitHub's mark drawn.
- Unknown names known as such.

### `main`

- Taking the next port when the first is busy.

### `markup`

*On strings.*

- `<Notes>` found with its line, none in code or comments; its text kept without its indentation; its `delivery` kept; one whose attributes can't be read keeping its text and no delivery; an unclosed one running to the end of the page.

### `nav`

- Links and `###` groups read from the Topics section only.
- The tree stopping at a cycle.
- A Topics entry outside the site or to a missing file is a problem.

### `drive::api_types`

- Every refusal the server answers — not found, unauthorized, unprocessable, a failure — read back by the client as the same refusal.

### `drive::client`

*Every server command runs against a real `devpage server` on a `file://` bucket, with each request it hears recorded.*

- A broken link stopping `publish` before any request, in text and `--json`.
- `publish` remembering the document per context in `published.yml`, adding a version the second time, the bucket keeping the zip it sent and its link rendering the new version; `--id` publishing to that document, `--new` creating one, the two refused together, and an id the server doesn't have pointing to `--new`.
- Contexts added, used and listed, the current one marked; `DEVPAGE_CONTEXT`; no context saying how to add one; a token command's output sent as the bearer token and stored nowhere.
- `list`, `versions` and `comments` printing what the server answers, their `--json` the server's own JSON unchanged; a detached thread marked with the version that detached it, not the latest; `reply` and `resolve` reaching the thread; a design's `download` its PNG; `delete` of a version and of the document; an unknown document the server's not-found refusal, naming the server.
- The threads as text: grouped by page, each thread's quote in its context, its messages with their authors aligned; every command parsing as the reference writes it.

### `drive::pdf`

*And in Chrome, in `drive::browser_tests::pdf`.*

- A site refused.
- The fixture doc's print call: A4, its header, and its footer with Chrome's page-number fields.

In Chrome:

- The fixture doc printed on A4.
- A doc embedding a design with problems printed without waiting for it.
- The devpage talk one 16:9 page per slide.
- A printed doc showing each widget's first render, terminal lines wrapping on paper, and its diagram on paper with its node's link in the PDF.

### The page script

*All in Chrome, in `drive::browser_tests::page_script`.*

- The logo loaded at 22px before the navbar's title and centred on it; the tab's icon is the head's, under [`page`](#page).
- The navbar's dropdown by click and keyboard, its highlight following an in-place link.
- The theme selector following the system until clicked, then switching every token and remembering the theme across a reload.
- A diagram, served drawn, its shapes taking the theme's `--surface` in light and again in dark after the switch, and its node's link opening the page in place.
- The navbar folding into a menu on a narrow screen.
- A deck's keys; a slide change, and back, fetching nothing with a slide shown in every frame.
- A doc's outline click scrolling without a fetch.
- A deck served with comments off showing no comment UI, even with comment mode remembered on, and <Keys>C</Keys> doing nothing.
- A doc's outline above the text on a narrow screen.
- Every system widget rendering, long lines scrolling, task lists without bullets and an inline widget keeping its paragraph.
- Widgets in a doc and a deck, a page loading only the widgets it uses.
- A plain page fetching no widget but its sidebar; on a cold load of a page with widgets, a helper, a diagram and a design, every module asked for by the head, none by a script, each once, and nothing fetched for the diagram; a page loaded in place bringing its preloads.
- Widgets mounting again after a live update and an edited widget or helper reaching open pages, a widget rendered by name inside an unchanged one too.
- A reload revalidating the page and what may change (`304`) and asking for nothing content-addressed, counted from the first load's last answer to the reload's.
- A live update finding its page unchanged leaving the DOM untouched, and one that changed it swapping it.
- A live update keeping an unchanged widget's node and state and the scroll, with no frame showing an unmounted placeholder; an edited widget mounting afresh and its old root unmounted.
- Live updates during a load making one more load after it, not one each.
- A link swapping in a page built off-screen, at the top, and back and forward restoring each page's scroll.
- Comments asked for each page source only — never a widget — all at once.
- A comment anchored inside a widget.
- A broken helper's message shown in place, and a project `Callout.tsx` leaving the system `Callout` rendering; an unknown tag's error is the served HTML's, under [`widgets`](#widgets).
- The sidebar at first paint collapsed with the branch to the page open, its search box drawn, and React taking it over without changing a node, its markup or any box — after a reload too, with the branches the reader opened open from the first frame; React rendering the server's markup; a chevron opening and closing its entry.
- The sidebar staying mounted across links, back and a live update: the same element in the same place in every frame, never empty, highlighting the page the frame shows, the highlight moving in the swap's own commit, the branch navigated to opening and opened branches staying open, the tree edited by a live update reaching it, and the widget never fetched again.
- The sidebar's search box, its searches answered by a stand-in for the endpoint in its answer's shape: typing showing results in the tree's place, grouped by page — the page's row, its sections under it, the more row — the lede of a page found by its title, another document's title over its page, the snippet's words marked; the first row selected the page found by its title, else the best section; arrows moving through every row, round; Escape and the clear button clearing it and bringing the tree back. A section's result swapped in through the page swap, landing on its first matched word with the heading scrolled past — again when scrolled away — its words marked in the section and not in code, a widget, its heading or the next section, the box keeping its text; Escape on the page taking the marks out and leaving the text as it was; opening another section moving them, back taking them out; a doc's chapter loaded whole landing the same way. A page found by its title opening at its top with nothing marked, the page shown and another. `/`, ⌘K and Ctrl+K focusing it, the `/` not typed. A slow answer to earlier text never replacing a later one's results.

### `problem`

*No tests of its own:* a problem is tested where it is found.

### `page`

- A page's content holding its own links by file and line — a heading's anchor, a link, an address on the web, an embedded design — and none of the navbar the frame puts round it.
- `ids` as a Markdown file and a doc render them: a file's own, one chapter's unprefixed, several chapters' each under its chapter's name with the chapter's own, none for a deck or a missing file; each one an id the doc's page renders.
- The canvas page titled without `.design`, and carrying its mount.
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
- The navbar's links, dropdowns, icons and icon-only links rendered in order.
- The link holding the page shown highlighted, a dropdown holding it too.
- Missing files and unknown icons as problems.
- A target outside the served folder a problem, followed neither as a file nor as an address on the web.
- No navbar rendering nothing, the comment-mode switch in the corner, and the theme selector off with `theme-selector: false` while the switch stays; with comments off, no switch.
- A logo before the title and as the tab's icon, for a doc and a deck too.
- A missing logo as a problem and the page shown without it.

### `search`

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
- A save reading again only the pages it touches, a page changed unheard keeping its old text. An index kept current answering every query as one built fresh: after an edit, a page added first in the Topics, a chapter's new title, front matter changed, a rule excluding a deck and lifted, a chapter deleted and back, a folder deleted — and on the frozen docs after every page is saved in turn.
- Sections keeping the rendered ids — every one an id the check lets a link land on, inline code, a duplicate, a frame id, a link and a footnote in a heading among them — their text sorted into prose and code; snippets cut at sentences; code split into identifiers and parts; near matches counting a swap as one edit.

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

### The restore of a deploy

`tests/deploy_restore.rs`, two `--ignored` tests, one per kind of bucket:
each brings up a server from `deploy/server/compose.yaml` — building the image
from the checkout, with RustFS for S3 and Azurite for Azure Blob — publishes
two versions and starts a thread, deletes the server's volume, starts it on
an empty one, and finds both versions, the thread, the latest at the link and
version 1 unchanged: [the PVC is only a cache](../../viewer/server/constraints/the-pvc-is-only-a-cache.md),
on the example deploy. Without Litestream's restore it fails.

### `drive::http`

The design tests through the server — a PNG, its `304`, its theme, the watch stream and a design as the entry document — are listed under [`designs`](#designs), and the Chrome suites of the pages it serves — `drive::browser_tests::canvas` and `::comments` — under `designs` and `comments`.

- A widget's module, and a module that throws why one doesn't compile, sent as JavaScript, `immutable` only at its current address; `404` for one there is none of.
- Every built-in asset `immutable` at its content address, its bare name and a stale hash answering `no-cache`, and the page naming them.
- An asset sent as brotli or gzip as `Accept-Encoding` takes, each with its own ETag, and `304` for a holder of it.
- A page, an image and a document file answering `304` to a strong or weakened ETag until they change; the event stream carrying none.
- A page naming its widgets and carrying their placeholders, code left alone.
- `/` and the site's pages following the entry file's changes without a restart, through an invalid state and back.
- Comment threads over HTTP, against the in-memory store; each write reaching open pages without a watcher.
- A server and the watcher its app starts announcing a comment write once.
- A page's event stream carrying each change on its bus, in order.
- A search answering pages as JSON whether comments are on or off.
- A page served with its Mermaid diagram drawn in its HTML.
- A slide's PNG asked for inside a shot and not drawn yet refused with `508`, and `404` for a slide the deck doesn't have — past its last, before its first, or a page that isn't a deck — neither starting Chrome.
- Deployed writes needing the header and stamped with it, over HTTP and in the identity rule alone — trimmed, and a blank one refused.
- A thread on a page that isn't a file in the served folder refused with `422`.
- With comments off, every comment request answering `404`, no `.devpage/` created, and the page carrying the setting.
- Nothing outside the served folder served, through HTTP's percent-decoding.
- On a published document, a thread made on the page listed by the API on the page's own path; one started through the API marked on the open page; after a publish that removes its quote, that thread listed under *Detached comments* and the other still marked.

### `drive::server_mode`

*With its configuration's; in Chrome, in `drive::browser_tests::server_mode` and `::server_mode_pages`; its comments in Chrome are listed under [`comments`](#comments).*

- Every `DEVPAGE_*` variable read, the defaults filling what is left out — the cache's limit 80% of the volume — and each missing, malformed or foreign variable stopping the server by its name, never echoing its value; another command's variables no mistake.
- A published zip kept in the bucket byte for byte and served at its link — the latest in place at `/d/<id>/`, each version under its own mount — and a version kept unchanged by a later publish, its sha-256 recorded.
- A publish naming its document file with `?entry=`, served from it, and an entry outside the zip refused; a broken link or a zip entry outside the document refusing the publish with `422`.
- An upload over the limit refused with `413`, one over axum's own 2 MB default and under the limit accepted.
- With a user header, a write without it refused with `401` and each version recording its author; without one, every write `anonymous`.
- Documents listed and read with their versions; a document deleted from the store, the bucket and the cache; deleting the latest version making the one before it latest; an id that is no UUID naming no document.
- Concurrent publishes to one document taking one number each.
- A carry that fails publishing nothing: no version recorded, no zip left in the bucket, every thread where it was, and the next publish taking the same number.
- A deleted document leaving no change bus and no lock behind; a mount's path, `/d/<id>` or `/d/<id>/v/<n>`.
- With the cache deleted, every version rendering again from the bucket; a restarted server serving what its cache holds without the bucket, and deleting what a run left half-unpacked; no scratch left after a publish.
- Past its limit the cache evicting the least recently read version and a read fetching it again; misses arriving together unpacking a version once; a version bigger than the limit served without being cached, its folder deleted after; a version answering a request never evicted; an evicted version made again under both its mounts.
- A published page asking no external address.
- A thread started, replied to and resolved through the API and through the latest's page — the same threads, on the page's unmounted path, oldest first — a page outside the document refused, a second resolve finding nothing.
- With a user header, every comment write — the API's and the page's — needing it and carrying its author.
- Each publish carrying the open threads onto the new version: one kept, one with new context, one whose quote is gone and one whose page is gone detached, each still on the version it was started on; the latest's page listing the detached one as detached; a thread started then on the new version; an earlier version taking no comments; a detached thread staying detached when its words come back.
- Deleting a document deleting its threads.
- A thread started only on a file in the latest version, by the API and the page alike: a missing page, a folder and one that climbs out refused with `422`, a lone page no document links to accepted.
- A blank author header refused by both routes, and a padded one trimmed.
- A detached thread taking replies by its id, from the page and from the API; a made-up thread `404`.
- Carry reading each page as it is shown: a thread on bold text inside a widget, under front matter, staying attached.
- The front page saying how to connect, linking to the repository, carrying the logo and listing no documents; `/favicon.ico` being devpage's logo; a page told the version it shows, the latest and every version, version `n` pinned and the latest not.
- A new version of a design never answered with the old one's PNG or `304` at the document's link, and the old version still showing its own; a design's download its PNG, painted once and kept beside its version, gone with it; a site, or a missing version, with none; a download counted with its version, seeded so on restart, deleted when the version goes.
- A publish, or deleting the latest version, refreshing pages open at the latest — including one that loaded the new version — and never a version's own.
- In Chrome: a page at the link swapping in each new version, its version menu following; an old version's banner linking to the latest, its menu listing every version, and a publish leaving it unchanged; the banner above a doc's page, never over it, wide and narrow, and a long sidebar's last link above the version bar; a doc's download button saving its PDF, printed once and then read from the cache; every request a published page makes staying under its mount.

### `drive::slide_png`

*And in Chrome, in `drive::browser_tests::slide_png`.*

- Saving a file of one deck — its file, a slide — forgetting its slides and none of another deck's; saving what a deck embeds — an image, a design, an image the design shows, a widget, a new widget file — forgetting its slides, and so do adding an image the design shows that wasn't there and adding a font to the design's font folder; saving a page a slide only links to forgetting none of its deck's; a slide being drawn when a file it is drawn from changes, or before its files are known, not kept.

In Chrome:

- A slide served as a 1920 × 1080 PNG, each slide a different picture.
- A slide beside one with a design drawn in under 3 seconds, without waiting for the design; the design's own slide drawn with it painted.
- A slide showing its own PNG answered promptly.
- A repeat request answered from the cache — `304` for a holder of its ETag, the cached picture inside another shot — until a change, after which the slide is drawn again.
- Chrome closed when told, and started again for the next shot.
- A slide shot without its comments: drawn again after a thread is started on it, the same picture.

### `drive::validators`

- `If-None-Match` compared the weak way, as a list, and `*`.
- Brotli preferred over gzip, and an encoding with `q=0` refused.

### `drive::video`

*And in Chrome, in `drive::browser_tests::video`.*

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
- Every slide needing notes, the error naming the ones
  without; a deck without `audience` or `goal` refused, naming them; a site
  and a doc refused.

In Chrome:

- A two-slide deck recorded in Chrome for Testing with a synthetic voice fed
  through the same player at a short pacing — no GPT-Live — to an mp4 with a 1920 × 1080 video
  track and a sound track, the deck moving to slide 2 on the way.
- A recorded slide showing a slide's PNG getting it drawn, never refused as
  if asked for inside a shot.
- `tests/fixtures/narrated-talk` recorded end to end with GPT-Live on OpenAI,
  when `DEVPAGE_OPENAI_API_KEY` is set. The Azure path has no live test.

### `adapters::watch`

Counting the paths the platform's watcher hands the handler, before sifting,
on real folders. A test that changes what is watched waits for the re-plan
itself — the folder watched again, or no longer — never for a fixed pause,
and one that expects quiet saves something in sight after and waits for
that, so a loaded machine slows them down without failing them:

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

The watch set itself is the served folder's, tested under
[`folder`](#folder).

Through the listeners:

- A saved page refreshing open pages.
- Ignored files and files outside `watch` not refreshing.
- A comment thread always reaching its page; one the server wrote through the store announced once, by the server, and a hand edit of it once, by the watcher.
- The `watch` list following the entry file.
- A site's `watch` narrowing only its own folder, whichever document devpage was started with.
- Widget files refreshing whatever `watch` says.

Without waiting on file-system events, fed made-up `notify` events:

- Paths under `target/`, `.git/`, a `.gitignore`d folder and `.devpage/` dropped before any listener hears of them or any is canonicalized.
- A comment thread and a `.gitignore`d file a site's `watch` names still arriving, and open pages hearing the thread's page, then a refresh.
- A new `.gitignore` rule taking effect without a restart.
- A burst holding each arrival once, in the order it first arrived; a burst of saves to one file sending one `change`, a later burst another; and what the platform reports handed on in bursts.

### `watch`

*On real folders, without a platform watcher.*

Local mode's response to a burst of saved paths, `Listeners::changed`, ported from the watcher so its order is pinned on every platform:

- A saved page refreshing open pages before the search index reads it, the design store having forgotten the design saved in the same burst by then.
- A changed file reaching the design store, and none under `.devpage/`.
- A saved file reaching the slide PNGs before open pages hear of it, and none under `.devpage/`.
- A comment thread heard before the refresh it shares a burst with; with comments off, none announced.
- A new `.gitignore` rule taking effect without a restart, and saying the folders to watch may differ.
- The search index hearing of saved and deleted pages.

### `widgets`

- A system widget's address carrying its own version, each one's its own.
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

- `parse`: imports, attributes and text with props; JSX's strings without escapes, bare attributes and names with `-`; nesting and numbers kept within their limits, arrays and objects counting as levels and numbers too small named so; JSX whitespace in text; every syntax error, and each problem, at its line and column.
- `ids`: a first parse numbering elements in order; an insert at the front, a removal and an edit keeping every other element's id; keyed siblings keeping theirs when reordered; a matched element's children matched too and new ones numbered, a different root a new one; 100,000 siblings matched in a pass; the edit script a longest common subsequence; a list rewritten past the edit cap — every third row removed, shuffled, reversed, half replaced — keeping the id of every row that survived and giving none another's; gaps nested past the depth cap still giving every row an id of its own. A release bench, ignored, times rewritten lists at 1,000, 10,000 and 100,000 rows.
- `tree`: keys spread evenly sorting in order; a key placed between two — before the first, after the last, in the middle, 2,000 times over — always sorting between them and staying short; children inserted anywhere keeping their order, the same children placed otherwise the same.
- `resolve`: a `key` a string unique among its siblings, two instances of one file their own nodes; props and defaults; an import name unique and never a built-in element's; sizes and spacing never negative; `justify` naming all it takes; `x` and `y` in a frame with auto layout, a negative size, an attribute given twice and an import of something other than a design each a problem; `props` only on the root, and no `x` or `y` on it; `"fill"` only under auto layout; a colour with anything but hex digits in it a problem, not a panic; a tree nested too deep through instances; an instance taking its props and the attributes every element takes; values parsed into their types; opacity kept between 0 and 1; every problem reported at once and each at its place; a missing import a problem at its line, used or not; a design that uses itself; paths kept inside the served folder; an image that doesn't decode — a broken PNG, a truncated JPEG — a problem at its `src`, and a good one's pixels going with the scene; `clip` only on a frame; numbers inside a gradient, a `viewBox` and a `d` held to the same limits as any other; an interaction relative to the file that wrote it, one to nowhere a problem, and one only ever to a design; a valid design having none; a text set in the font its `font` names through the `themes.yml` files above it — a lower one adding and overriding names — or in Inter, one design mixing families; a `themes.yml`'s `fonts` a map of lowercase names to paths, anything else a problem at its line, and `font` no key of it; weight picking the face, and a scene carrying only the faces its texts use, once; a weight the font lacks a problem listing those it has, at the `weight` or at the `Text`; an unknown name a problem at the `font`, listing the names there are; every way a font can't be used — missing, outside the served folder, not a font, WOFF, WOFF2, a collection, over 4 MB, unreadable, a folder of no fonts, of two families, of two files of one weight, or holding a bad file — a problem at its `fonts` entry, once; a value given as a prop — `src`, `font`, an interaction — read where it was written, its problem there.
- `resolve::again`: random saves of a design — rows inserted at the front, middle and end, removed, moved, keyed and not, made instances and frames, their text edited, their frame's layout and fill, a component's attributes, a theme's colour — read by rows and through a component's `props` defaults — a line added above everything — each resolved again only as far as it reaches, never whole, its edits applied once to the scene its holder holds — the resolution keeps none — that scene a fresh resolve's and the patch bringing the browser's copy there, a problem every problem; an edit to one row's text and a row inserted before the first one operation each, resolving again the same elements at 100,000 rows as at 1,000 — the edited row's text, the inserted row's frame and text — counted, not timed; a component's save reaching only its instances' roots, a theme's only the nodes that read it, a component's tint from its `props` defaults among them; saves while a design has problems kept, and the fix bringing the last scene across all of them.
- `render`: an edit in every kind of frame — a list's widest row made wider, a list's own fill changed, a child of a frame with no layout resized and one inserted first, a sibling of a `"fill"` child resized, a child of a `"space-between"` frame resized, a fixed-width card of wrapping text in a frame with no layout made wider — visiting the same nodes at 1,000 siblings as at 10,000, none beside what changed, and leaving a drawing laid out and painted as a fresh one, with their numbers at 1,000, 10,000 and 100,000 in an ignored bench; a list's new `align` placing every row again and nothing in them, and every attribute that places a list's children — `align`, `justify`, `padding`, `gap`, its direction — changed from each value to every other on 1,000 rows, each leaving a drawing laid out and painted as a fresh one; a frame with no layout that grows laying out again a child whose size its box reaches; a row of fractional height inserted before the first of 10,000 visiting only the new row, a tile rounding only the rows it shows anew, and the drawing then a fresh one's, box for box and pixel for pixel; layout boundaries and runs — 400 random trees of rows, columns and frames with no layout, every `align` and `justify`, children fixed, hugging, stretching and filling, wrapping text, runs inside runs and boxes placed absolutely, at places in eighths of a pixel — laid out box for box as taffy lays the whole tree out, an edge differing only where taffy's own unrounded edge lies within a thousandth of a pixel of a rounding boundary; one row of a column edited — its text, its height, a row inserted before the first — visiting the same three nodes at 1,000 and 10,000 rows, the damage from the row to the column's end, and the drawing laid out, painted, hit-tested as a fresh one; a row inserted before the first of 1,000 or of 10,000 one insert in the patch and one chunk made, two nodes visited; random saves of a list — inserts at the front, middle and end, removals, moves, resizes, keyed and not — resolved from the saved text and patched onto the canvas's scene, every row the save didn't touch keeping its id and the drawing painting as a fresh one; auto layout, hug, fill, a root that fills on its own hugging, a drawing laid out as the layout lays it out, align and justify, and absolute placement; boxes — a box placed absolutely in a frame with no layout inside auto layout, measured from its own parent frame, and 300 random trees of auto layout and none at fractional places in eighths — rounded box for box as taffy rounds a whole tree; a hugging frame with no layout taking its children's extent, nested ones too; the drawing being the root plus what is painted outside it, cut by clipping, and a drawing 70,000 pixels wide painted whole; a design 100,000 pixels tall streaming its PNG, a tile at its bottom asking the index for a handful of commands; a click on it answered from the index, the topmost interaction, none where a clip hides it; an edit to one row damaging only that row, its tiles as a fresh drawing's; updates — a row growing, a clipping parent resizing, a row added, a child past the top-left, rows removed — painting as a fresh drawing, the area's new strip damaged, a moved corner damaging all; an edit to one row's text visiting only its path — three nodes — on 1,000 rows and on 10,000; an edit applied as a canvas applies it — the patch onto its scene, the drawing by what the patch replaced — costing as much on a balanced design of 10,000 rows as on one of 100; a row that grows moving only the rows after it; random edits of every kind to the checkout design, each applied by its patch as a canvas applies it — attributes, text, sizes, layouts, clips, opacities, children added, removed, moved to the front, the root replaced — each leaving a drawing laid out, painted and holding property-tree nodes as a fresh one; the examples painted tile by tile within 2 of 255 of their references, and a tile the same part of one paint of the whole; no input — hostile values for every attribute of every element, a file cut at every character — making the engine panic; text hugging and centered; fills, gradients along their angle, strokes inside the box, per-corner radius, a shadow following each corner's radius and the spread, sharp and blurred, and its edge the Gaussian at every box size from 100 to 16,000 — fading out with no step, rising 10–90% over 2.563 σ; clip and opacity as one group; vectors scaled into and hugging their view box; images covering their box, PNG and JPEG decoding; images too many for the GPU's atlas a problem at the `src` of the first that doesn't fit, one file shown again taking no more room; a drawing painting the same images as the last painting the last one's blobs, and one whose files, order or shadows differ, or made after none, fresh ones; the example designs painting as their reference PNGs, through tiles; a text laid out in its font, and in a variable one at its weight's point on the axis, which its glyph runs carry; a renderer that drew other fonts laying out as a fresh one, holding one scene's font files.
- `scene`: edits between two scenes — an insert before the first of 100,000 one insert; moves, removes and changes found by id, a row moved out of its parent a new one; a new root replacing the scene — applied as they say, the files' counts and the id index following; edits that can't be applied whole — an id the scene hasn't, a sibling under another parent, a node it has already, a move under itself — changing nothing after the edits before them did; edits carrying only the files the scene lacks, and a file no node uses any more dropped; the longest run of siblings still in order staying; a file larger than 4,096 a side resampled once to fit, by area, a transparent pixel lending no color.
- `wire`: a patch saying what it changed, by id, and carrying only that; edits of every kind coming back from their patch the same; a patch that isn't one — an empty operation, an insert with no node, a node with no id — refused before anything is applied; a scene and a patch carrying both themes; an image file sent once however many nodes show it, a patch carrying only the files the browser lacks, and one whose nodes show a file never sent refused; a font file the same, the browser keeping the one it checked.
- `fonts`: a font file knowing its family, its weight or its weight axis; a folder's italic faces passed over, and one of italic faces only a problem.
- `atlas`: four 4,096-pixel squares filling the atlas at their places, a fifth not fitting.
- `prefix_sums`: random inserts, removes and replacements agreeing with a sorted map on every prefix, search and walk; an insert before the first of 100,000 and a prefix at the end in log time.
- `display`: the extent of the chunks' covers knowing its least and greatest edge as they come and go.
- `compare`: the comparison forgiving anti-aliased edges only, and catching a missing divider or icon on the checkout design.
- `png`: the band writer holding one row of tiles at a time and writing their rows in order, a tile it can't have stopping it; a band holding as many rows as fit in 16 MB, a whole row of tiles up to 16,384 pixels wide, at least one row however wide; a drawing 1,000,000 pixels wide written in strips holding at most 16 MB at once, its pixels right at both ends and across a strip's seam and a band's; a strip painting as the part of the tile it replaces.
- `design`: a new design staling every tile of both themes, an edit damaging only where it changed, from the drawing's top-left, in each theme drawn and no other, the drawing then painting tile by tile as a fresh one; an edit leaving the scene too many images for the GPU's atlas saying why, damaging no tile and showing the last good drawing — its size, its images and every tile a view needs, a pan's and a zoom's never painted before included, through later saves — and the edit that mends it damaging every tile and painting as a fresh drawing; a whole scene that can't be drawn after one that could showing the one before; a whole scene that can't be drawn from the start tried again only once it changes; a whole scene redrawing both themes, and a theme let go left alone.
- `tiles`: a view needing the tiles it crosses, its middle first, and a tile it doesn't show landing nowhere; the tile cache holding its cap, the least recently used dropped first; damage making exactly the tiles it overlaps stale at every level, a stale tile still there until painted again; the frame plan — the JavaScript compositor's rules, ported — asking for the missing tiles middle first while fewer than its budget are in flight, and after arrivals only as many as arrived; an unsettled frame asking for nothing and counting what it lacks; an answer to a ticket never given, answered already or given by another plan dropped, and a tile painted as nothing asked for again under a new ticket; a tile damaged in flight arriving stale, kept and asked for again, damage elsewhere or in the other theme leaving it current; every painting let go — past the cap, or replaced — evicted once; a frame drawing coarse levels under fine and the view's own on top, each where it lands.
- The crate: the core naming no adapter and no rasterizer.
- `shadow`: a shadow in pieces matching it as one image, with no seam and within the bound for how wide its blur is on the screen, at fractional scales, for every kind of piece, a box thinner than its blur, and a dozen random outlines.

### `devpage-design-wasm`

- A scene sized, painted and hit-tested.
- A design in a font file, by its scene and by a patch bringing another weight, laid out and painted as the server paints its PNG, pixel for pixel.
- The engine's atlas model and vello's own resolver, as the module ships it, agreeing on 4,000 random sets of images: whether they fit, the first left out, and each one's place.
- The two agreeing on a real drawing's image files and shadows, replayed through the one paint entry point as a tile is; the same design updated to too many files showing the last good drawing, its images placed as vello places them.
- A patch before any scene, or garbage, doing nothing.
- Problems keeping the scene until a patch clears them.
- A theme switch painting the scene again in the other theme.
- A click landing only on what a clip shows.
- The damage of each message reaching the canvas's frame plan in the module: a whole scene staling every tile of both themes, a patch only the tiles over where the node it changed was and is, in each theme drawn, and a theme switched back to keeping its drawing and its tiles.
- A patch leaving the design too many images for the GPU's atlas a problem, as the stream's are, the plan still showing the tiles painted, staling none and asking for none, and a zoom's tiles, never painted, painting the last good drawing; the patch that mends it staling every tile and painting as the server's PNG, pixel for pixel.

### `devpage-design-module`

- The vendored module built from the sources as they are.
- The fingerprint changing with an engine source, the `wasm` profile and a locked version in the module's tree, but not with a package outside it.

## What has no test

<Callout kind="note" title="Not covered by any test">

syntax highlighting except through the `page` tests; a canvas reopening
its stream after devpage restarts, closing streams in a hidden tab, and
drag-to-pan and pinch on the canvas; chrome-headless-shell's and Chrome for
Testing's first download (it needs an empty cache folder); GPT-Live on Azure;
`devpage video` stopping on a signal; fullscreen on a deck (headless Chrome refuses it without a real user
gesture); and how the light and dark themes look, which is checked by eye.

</Callout>
