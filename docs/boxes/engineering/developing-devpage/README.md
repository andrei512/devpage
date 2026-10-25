# Developing devpage

How to work on devpage itself: build it, run its tests and the three gates,
run the documentation while you edit it, and know what needs a rebuild. It
also maps the source — which module does what, and how `main` wires them — and
says what the test suite covers and what it doesn't. It is a task article
under [Engineering](../README.md); how a change travels from idea to `main` is
the [SDLC](../sdlc/README.md).

You need the toolchain from [Installing devpage](../../../installing.md#what-you-need).
Every command runs from the repository root.

## Build and run

```sh
cargo build                 # debug build, target/debug/devpage
cargo run -- docs           # build if needed, then serve these docs
```

Building devpage builds no WebAssembly: the
[design engine](../../viewer/design-engine/README.md)'s module for the browser
is [vendored](#the-vendored-design-engine) in `assets/vendor/`, and the binary
embeds it.

Run it from the repository root: the folder devpage starts from is the
[served folder](../../viewer/writing-a-document/README.md#the-served-folder),
so the whole repository is served and the docs' links to `../LICENSE` and to
the examples in `examples/` work. `cargo run -- <args>` passes everything after `--` to
devpage, so `cargo run -- check docs` and `cargo run -- pdf examples/sync-design`
work the way an installed `devpage` would. It prints the address —
`http://localhost:3030`, or the next free port up if another devpage already
holds 3030.

### The vendored design engine

`assets/vendor/design-engine.wasm` and `assets/vendor/design-engine.js` are
the engine built for the browser and its bindings, committed like the pele
bundle. Rebuild them after changing anything they are built from:

```sh
cargo run -p devpage-design-module
```

It builds `devpage-design-wasm` for `wasm32-unknown-unknown` in the
size-optimised `wasm` profile, into `target/design-module/`, runs wasm-bindgen
on it as a library, and writes the two files and
`assets/vendor/design-engine.fingerprint`
([Dependencies](../dependencies/README.md#devpage-design-module-the-modules-builder)).
The target is the one `rust-toolchain.toml` names, so rustup adds it in a
checkout; nothing else needs it. The fingerprint is a SHA-256 of everything
the module is built from: every file in `design/src/`, `design/fonts/`,
`design/proto/`, `design/wasm/src/` and `design/module/src/`, the three crates'
`Cargo.toml` and `design/build.rs`, the `wasm` profile, and the locked version
of every package in the module's and wasm-bindgen's dependency trees. The
toolchain is recorded beside it but not hashed, so the same sources give the
same fingerprint on every machine. `cargo test` fails while the recorded
fingerprint differs from the sources', and says to run the command.

## The edit–save–refresh loop

What you edit decides what you have to do to see it:

| You change | What it takes |
| --- | --- |
| A Markdown page under `docs/` | Save. Pages are read on every request and the open page [updates itself](../../viewer/live-updates/README.md). |
| A page's Topics list | Save. The sidebar is rebuilt on every request, on every open page. |
| `docs/devpage.yml`, or any document file | Save. Document files are read on every request and the index of them is kept current by the watcher. |
| Rust code in `src/` | Stop devpage (Ctrl-C) and `cargo run` again, then reload the browser tab. A page reconnects to its event stream by itself, but doesn't fetch itself again on reconnecting. |
| `assets/style.css`, `assets/devpage.js`, `assets/design.js`, `assets/vendor/pele.js` | Rebuild and restart — they are compiled into the binary with `include_str!` — then do a full reload of the tab. Live updates and in-place links swap only the page content and its navigation, never the stylesheet or the scripts. |
| A `.design` file, or an image a design uses | Save. The design store resolves again the designs made from that file and the open canvases redraw in place ([Designs](../../viewer/designs/README.md)). |
| The engine in `design/src/`, the module in `design/wasm/src/`, or `design/proto/design.proto` | Run `cargo run -p devpage-design-module` to rebuild the [vendored module](#the-vendored-design-engine), then rebuild and restart devpage, and do a full reload of the tab — the binary embeds the module. After changing what the engine draws, run `DEVPAGE_UPDATE_REFERENCES=1 cargo test -p devpage-design` to rewrite the reference PNGs in `design/testdata/`, and look at them before committing. |

## Tests

```sh
cargo test                  # 123 tests in the four crates; needs no network and no Chrome
cargo test -- --ignored     # the 25 tests that drive chrome-headless-shell
```

`cargo test` runs every crate in the workspace: 71 in `devpage`, 47 in the
engine `devpage-design`, 3 in `devpage-design-wasm`, whose tests run
natively, and 2 in `devpage-design-module`. Tests are written as `#[cfg(test)]` modules beside the code they
test — the canvas's Chrome tests in `src/designs/canvas_tests.rs`, a child
module of `designs`; there is no `tests/` folder. Route tests start the real router on a free port on
`127.0.0.1` and talk to it with `reqwest`; external-link tests point the
checker at a local web server, so the suite needs no network. The 25 tests
marked `#[ignore]` drive chrome-headless-shell through chromiumoxide — the
copy [`devpage pdf` downloads](../../viewer/pdf/README.md#the-chrome-it-drives)
into devpage's cache folder, fetched by the first test that needs it — never
a Chrome installed on the machine, so a run puts nothing in the macOS Dock.
They run in parallel. Run them with `--ignored` after changing the page
script, the design script, the module, the stylesheet, print mode or
`devpage pdf`. The canvas tests start it with `--enable-unsafe-webgpu` and
`--enable-gpu`: the shell keeps its GPU process off without the second, and
then a screenshot holds black where a WebGPU canvas is; with it, WebGPU gets
the machine's real adapter and the screenshot is the canvas as a reader sees
it.

**The watcher's tests need file-system events.** The six `watch` tests,
`server`'s test of the entry following its file, and the Chrome test of a
selected-text comment save a file and wait for devpage to hear of it through
`notify`. Where the platform delivers no events to the test process —
`notify` itself receives nothing, as can happen in some sandboxed or
restricted sessions on macOS — those tests fail while everything else passes.
That says nothing about the change under test; run them where file events
work before calling the suite green.

| Module | Tests | Ignored | What they pin |
| --- | --- | --- | --- |
| `check` | 10 | | Missing files and headings and links climbing out of the served folder; a slide number the deck doesn't have; a heading in a doc checked against the ids the doc renders; Topics checked against the page's own site, and a page in no site checked as a doc; a broken linked document named by its address, not an absolute path; each external address asked once and every link to it reported; the preview's background check refreshing open pages; the examples and devpage's own docs free of broken internal links. |
| `comments` | 4 | 4 | A thread's whole life leaving `git status` clean; a thread file that doesn't parse named and left alone; the local author without a `.git` folder; timestamps. In Chrome: selected text becoming a thread in the margin and the store, then detaching; a paragraph comment on a nested list anchoring again after a reload; **C** on a deck; a live update with the comments shown swapping the page in place rather than reloading it. |
| `designs` | 11 | 17 | The store announcing only the designs made from a changed file, and resolving a design again when a missing import appears; no design file — a pasted zero-width space in a colour, deep nesting, a huge number, a truncated file — stopping the store, which still sees saves afterwards; nothing outside the served folder read; a PNG, or `422` and the problems, and `404` for a design outside the served folder or missing; a design as the entry document; the canvas page titled without `.design`; the watch stream over gRPC-Web sending the scene, a patch of only what changed, nothing for a no-op, problems, and the end of a deleted design; one stream over several designs sending each scene once, patches naming their design, and going on after one is deleted; `devpage check` naming a broken embedded design's `file:line:col`. In Chrome: the canvas on the GPU and on the CPU each matching the PNG by the engine's comparison, over the whole drawing and a margin of plain page around it; a click navigating, back returning, an overlay opening and closing; an interaction inside an instance from another folder opening its own target; the wheel zooming about the pointer and panning; an undone typo clearing the problems; a deleted design no longer asking for its stream; a canvas saying so when the engine doesn't load; an embedded design drawing at its own size and surviving a page edit; an embedded design centred at its own size and only ever scaled down, to the page's width or the window's height; the stage's link opening the design it shows on its canvas page; a small design not enlarged on its canvas page; a doc printing its embedded design; eight embedded designs painting through one stream while the page still loads; a doc with eight embeds printing as fast as one with one; leaving the last design in place closing the stream; an embed with problems surviving a page refetch. |
| `document` | 7 | | Each kind parsed; bad kinds, fields and missing files rejected; a Markdown file as a one-chapter doc; the served folder as boundary and address root; a document outside it refused; a site's `watch` list; only YAML with a devpage `kind` is a document file. |
| `index` | 4 | | Document files found outside what `.gitignore` excludes; the nearest site above a page; nested `.gitignore` files and `.git` folders out of sight; changes keeping it current. |
| `main` | 1 | | Taking the next port when the first is busy. |
| `nav` | 3 | | Links and `###` groups read from the Topics section only; the tree stopping at a cycle; a Topics entry outside the site or to a missing file is a problem. |
| `pdf` | 2 | 4 | The default file name made safe; a site refused. In Chrome: the example doc on A4 with its header and footer; the example deck one 16:9 page per slide; a deck's keys; a doc's outline above the text on a narrow screen. |
| `render` | 13 | | An image of a design becoming its canvas, and a raw `<img>` found whole with or without its closing slash, its attributes read however they are quoted, spaced or capitalised; raw HTML kept and Mermaid fences marked; highlighting without a language label; unknown languages left plain; tables and heading ids; a doc's chapters with its outline; a deck's slides; links kept for chapters and slides from other folders; a one-file doc's title; a page's title. |
| `server` | 10 | | A site's page printing as a doc of one chapter; `/` and the site's pages following the entry file's changes without a restart, through an invalid state and back; comment threads over HTTP; deployed writes needing the header and stamped with it, over HTTP and in the identity rule alone; a thread on a page outside the served folder refused; serving a folder with no `.git` by what each file is; nothing outside the served folder served; a served folder reached through a symbolic link; each site's title linking to its own root page. |
| `watch` | 6 | | A saved page refreshing open pages; ignored files and files outside `watch` not refreshing; a comment thread always reaching its page; the `watch` list following the entry file; a site's `watch` narrowing only its own folder, whichever document devpage was started with. |

The engine's tests pin, in `devpage-design`:

| Module | Tests | What they pin |
| --- | --- | --- |
| `parse` | 6 | Imports, attributes and text with props; JSX's strings without escapes, bare attributes and names with `-`; nesting and numbers kept within their limits; JSX whitespace in text; every syntax error, and each problem, at its line and column. |
| `resolve` | 16 | Props and defaults; `props` only on the root; `"fill"` only under auto layout; a colour with anything but hex digits in it a problem, not a panic; a tree nested too deep through instances; an instance taking its props and the attributes every element takes; values parsed into their types; opacity kept between 0 and 1; every problem reported at once and each at its place; a missing import a problem at its line, used or not; a design that uses itself; paths kept inside the served folder; an interaction relative to the file that wrote it, and one to nowhere a problem; a valid design having none. |
| `render` | 24 | Auto layout, hug, fill, align and justify, and absolute placement; a hugging frame with no layout taking its children's extent, nested ones too; the drawing being the root plus what is painted outside it, cut by clipping, and a drawing too big being a problem; no input — hostile values for every attribute of every element, a file cut at every character — making the engine panic; text hugging and centered; fills, gradients along their angle, strokes inside the box, per-corner radius, clip and opacity as one group; vectors scaled into and hugging their view box; images covering their box, PNG and JPEG decoding; the example designs painting as their reference PNGs; the comparison forgiving anti-aliased edges only, and catching a missing divider or icon on the checkout design. |
| `wire` | 1 | A patch carrying only the nodes that changed. |

and in `devpage-design-wasm`, 3: a scene sized, painted and hit-tested; a
patch before any scene, or garbage, doing nothing; problems keeping the scene
until a patch clears them; and in `devpage-design-module`, 2: the vendored
module built from the sources as they are, and the fingerprint changing with
an engine source, the `wasm` profile and a locked version in the module's
tree, but not with a package outside it.

Tests that need files write them into folders under the system temp
directory, named with the test process's id, and don't all remove them.

**What has no test:** syntax highlighting except through the `render` tests;
a canvas reopening its stream after
devpage restarts, closing streams in a hidden tab, and drag-to-pan and pinch
on the canvas;
chrome-headless-shell's first download (it needs an empty cache folder);
fullscreen on a deck (headless Chrome refuses it without a real user
gesture); and the light and dark themes, which are checked by eye.

## The gates

Green before any pull request opens (from `AGENTS.md`):

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Formatting must match `rustfmt`'s defaults, clippy's warnings — on tests too —
are errors, and the suite passes. There is no CI yet, so nothing runs these
for you: run them by hand before you push. See
[CI](../sdlc/README.md#ci).

## The code

devpage is one binary — `main.rs` and eleven modules — with six built-in
assets, and the [design engine](../../viewer/design-engine/README.md) in three
workspace crates beside it: `design/` (`devpage-design`), `design/wasm/`
(`devpage-design-wasm`) and `design/module/` (`devpage-design-module`, which
vendors the module).
Each part of the diagram opens the page that explains it.

```mermaid
flowchart LR
  main["main.rs<br/>commands, startup"] --> document["document.rs<br/>kinds, served folder"]
  main --> index["index.rs<br/>document files, .gitignore"]
  main --> watch["watch.rs<br/>file watcher"]
  main --> server["server.rs<br/>routes"]
  main --> pdf["pdf.rs<br/>Chrome prints"]
  main --> check["check.rs<br/>links"]
  server --> render["render.rs<br/>frames"]
  server --> comments["comments.rs<br/>comment store"]
  render --> nav["nav.rs<br/>Topics → tree"]
  render --> highlight["highlight.rs<br/>code fences"]
  server --> designs["designs.rs<br/>design store, stream, PNG"]
  watch --> designs
  designs --> engine["design/<br/>the design engine"]
  server --> assets["assets/<br/>style, scripts, pele,<br/>design engine module"]
  click main href "#how-main-wires-it-together" "How main wires it together"
  click document href "../../viewer/configuration-reference/README.md#document-files" "Document files"
  click index href "../../viewer/serving-pages/README.md#which-document-a-file-belongs-to" "Which document a file belongs to"
  click watch href "../../viewer/live-updates/README.md#the-watcher" "The watcher"
  click server href "../../viewer/serving-pages/README.md#routes" "Routes"
  click pdf href "../../viewer/pdf/README.md" "Printing to PDF"
  click check href "../../viewer/checking-links/README.md" "Checking links"
  click render href "../../viewer/serving-pages/README.md#rendering-a-markdown-file" "Rendering a Markdown file"
  click comments href "../../viewer/comments/README.md#the-comment-store" "The comment store"
  click nav href "../../viewer/navigation-tree/README.md" "Navigation tree"
  click highlight href "../../viewer/serving-pages/README.md#rendering-a-markdown-file" "Code highlighting"
  click designs href "../../viewer/design-engine/README.md#in-the-server" "The design engine in the server"
  click engine href "../../viewer/design-engine/README.md" "The design engine"
  click assets href "../../viewer/serving-pages/README.md#built-in-assets" "Built-in assets"
```

| Module | Does |
| --- | --- |
| `src/main.rs` | The command line (clap): serving, `check` and `pdf`; wires the parts together and picks the address. |
| `src/document.rs` | `Document`: loads a document file of any kind, or a Markdown file as a one-chapter doc; the served folder and path safety; a site's `watch` patterns. `Entry`: the entry document, re-read when its file changes, keeping the last valid one while the file is invalid. |
| `src/index.rs` | The served folder's index of document files and of what `.gitignore` files exclude; finds a page's nearest site. |
| `src/watch.rs` | Watches the served folder, keeps the index current, sends `change` or `comments /<page>` on the change channel, and hands every changed path to the design store. |
| `src/designs.rs` | The design store — every design asked for, resolved in memory with the files it read, resolved again only when one of them changes; the `Designs` gRPC service and its watch stream; the PNG route; the canvas page; design problems for `devpage check`. |
| `src/server.rs` | The routes — the gRPC-Web watch route among them — request logging, the event stream, the comment requests and identity, and the built-in assets. |
| `src/render.rs` | A page, a doc or a deck inside its frame; an image of a `.design` file turned into its canvas; print mode; the problems bar; an invalid document file's page. |
| `src/nav.rs` | Reads Topics sections and builds a site's sidebar tree. |
| `src/highlight.rs` | Highlights fenced code for comrak, and marks Mermaid blocks. |
| `src/comments.rs` | The comment store: one YAML file per thread under `.devpage/comments/`, and the local author. |
| `src/check.rs` | Link checking, for the preview and `devpage check`, including external addresses. |
| `src/pdf.rs` | `devpage pdf`: serves the document internally and has chrome-headless-shell print it once every embedded design has painted, downloading the shell the first time. |
| `design/src/` | The engine: `parse` (text → syntax tree), `resolve` (→ scene), `render` (layout, display list, the CPU backend, PNG), `wire` (scenes and patches as messages), `proto` (the generated messages, and with the `server` feature the `Designs` service). |
| `design/wasm/src/` | The module: `lib.rs`, one `Design` per canvas — apply, lay out, paint on the CPU, hit-test; `gpu.rs`, the GPU backend on WebGPU. |
| `design/module/src/` | The module's builder: `main.rs` builds and vendors it, `lib.rs` computes and checks the fingerprint. |

What each crate does is in [Dependencies](../dependencies/README.md).

### How main wires it together

`main` in `src/main.rs` parses the command line. `devpage check` loads the
document, checks it and exits with `0` or `1`; `devpage pdf` loads it and
prints it. Serving starts in this order; a failure at any step stops devpage
with an error before anything is served.

```mermaid
flowchart LR
  load["1. Load the document<br/>Document::load"] --> index["2. Build the index, create the design store<br/>Index::build, Designs::new"]
  index --> channel["3. Create the change channel<br/>broadcast, 16 messages"]
  channel --> watcher["4. Start the watcher<br/>watch::start"]
  watcher --> bind["5. Bind the address<br/>--host, --port or 3030 up"]
  bind --> serve["6. Print the address, serve<br/>server::router_with_index"]
  click load href "../../viewer/configuration-reference/README.md#document-files" "Document files"
  click index href "../../viewer/serving-pages/README.md#which-document-a-file-belongs-to" "The index"
  click channel href "../../viewer/live-updates/README.md#the-channel-and-the-stream" "The channel and the stream"
  click watcher href "../../viewer/live-updates/README.md#the-watcher" "The watcher"
  click bind href "../../viewer/serving-pages/README.md#the-address" "The address"
  click serve href "../../viewer/serving-pages/README.md#routes" "Routes"
```

1. **Load the document** from the path argument, or `.`, inside the served
   folder — the current folder or `--root`. It becomes the `Entry`, shared by
   the watcher and the server, which re-reads it whenever its file changes.
2. **Build the index** of the served folder's document files and `.gitignore`
   files; the watcher and the server share it. Then **create the design
   store**, empty: it resolves a design the first time one is asked for, and
   the watcher and the server share it too.
3. **Create the broadcast channel** that carries `change` and
   `comments /<page>` messages, and hand it to the link checker, which sends
   `change` when a background check of an external address finds it broken.
4. **Start the watcher** on the served folder, with the design store to hand
   changed paths to. `main` keeps it for the whole
   run; dropping it would stop watching.
5. **Bind** exactly `--port`, or the first free port from 3030 up, on `--host`
   (`127.0.0.1` by default). The identity is local — `git config user.name` —
   on a loopback address, and the `--user-header` header otherwise.
6. **Print `<title> → http://<host>:<port>`** (`localhost` for a loopback
   address) and serve until devpage is stopped.

## Working on the docs

These docs are a devpage site: `docs/devpage.yml`, rooted at
`docs/README.md`. Keep `cargo run -- docs` open, from the repository root,
while you write, and run `cargo run -- check docs` before you push — `cargo
test` checks the same internal links.
How to write and organize pages is in the
[viewer's task articles](../../viewer/README.md#topics); the reading hierarchy
this repository follows is the documentation skill,
`.claude/skills/documentation/SKILL.md`. Check every diagram you add with the
Node command in [Diagrams and themes](../../viewer/diagrams-and-themes/README.md#writing-a-diagram).
