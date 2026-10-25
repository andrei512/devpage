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

Run it from the repository root: the folder devpage starts from is the
[served folder](../../viewer/writing-a-document/README.md#the-served-folder),
so the whole repository is served and the docs' links to `../LICENSE`
work. `cargo run -- <args>` passes everything after `--` to
devpage, so `cargo run -- check docs` and `cargo run -- pdf docs/talks/devpage`
work the way an installed `devpage` would. It prints the address —
`http://localhost:3030`, or the next free port up if another devpage already
holds 3030.

## The edit–save–refresh loop

What you edit decides what you have to do to see it:

| You change | What it takes |
| --- | --- |
| A Markdown page under `docs/` | Save. Pages are read on every request and the open page [updates itself](../../viewer/live-updates/README.md). |
| A page's Topics list | Save. The sidebar is rebuilt on every request, on every open page. |
| `docs/devpage.yml`, or any document file | Save. Document files are read on every request and the index of them is kept current by the watcher. |
| Rust code in `src/` | Stop devpage (Ctrl-C) and `cargo run` again, then reload the browser tab. A page reconnects to its event stream by itself, but doesn't fetch itself again on reconnecting. |
| `assets/style.css`, `assets/devpage.js`, `assets/vendor/pele.js` | Rebuild and restart — they are compiled into the binary with `include_str!` — then do a full reload of the tab. Live updates and in-place links swap only the page content and its navigation, never the stylesheet or the scripts. |

## Tests

```sh
cargo test                  # 90 tests; needs no network and no Chrome
cargo test -- --ignored     # the 20 tests that drive the installed Chrome
```

Tests are written as `#[cfg(test)]` modules beside the code they test; the
`tests/` folder holds only fixtures — `tests/fixtures/sync-design`, a doc of
three chapters that the doc and PDF tests load. Route tests start the real router on a free port on
`127.0.0.1` and talk to it with `reqwest`; external-link tests point the
checker at a local web server, so the suite needs no network. The 20 tests
marked `#[ignore]` drive the installed Chrome through chromiumoxide — run them
with `--ignored` after changing the page script, the stylesheet, a widget,
print mode or `devpage pdf`.

| Module | Tests | Ignored | What they pin |
| --- | --- | --- | --- |
| `check` | 15 | | A site's navbar reported once and its links followed; a missing logo reported once; widget problems — an unknown widget tag, a project widget with a system widget's name, a widget or a helper that doesn't compile, a widgets folder's problems reported once however many pages; missing files and headings and links climbing out of the served folder; a slide number the deck doesn't have; a heading in a doc checked against the ids the doc renders; Topics checked against the page's own site, and a page in no site checked as a doc; a broken linked document named by its address, not an absolute path; each external address asked once and every link to it reported; an unreachable address asked again before `devpage check` reports it; the preview's background check refreshing open pages; an unreachable address shown only after three failures in a row and cleared when it answers; the fixture doc, the devpage talk and devpage's own docs free of broken internal links. |
| `comments` | 4 | 5 | A thread's whole life leaving `git status` clean; a thread file that doesn't parse named and left alone; the local author without a `.git` folder; timestamps. In Chrome: commenting off until comment mode is on, a comment made, the switch remembered after a reload; selected text becoming a thread in the margin and the store, then detaching; a paragraph comment on a nested list anchoring again after a reload; the comment icon and a wide table staying inside 390 px and 820 px windows; **C** on a deck. |
| `document` | 8 | | Each kind parsed; a navbar read in order with its icons and dropdowns, `sidebar` and `theme-selector`, and its bad shapes refused; bad kinds, fields and missing files rejected; a Markdown file as a one-chapter doc; the served folder as boundary and address root; a document outside it refused; a site's `watch` list; only YAML with a devpage `kind` is a document file. |
| `icons` | 1 | | Lucide icons and GitHub's mark drawn; unknown names known as such. |
| `index` | 4 | | Document files found outside what `.gitignore` excludes; the nearest site above a page; nested `.gitignore` files and `.git` folders out of sight; changes keeping it current. |
| `main` | 1 | | Taking the next port when the first is busy. |
| `navbar` | 6 | | Links, dropdowns, icons and icon-only links rendered in order; the link holding the page shown highlighted, a dropdown holding it too; missing files and unknown icons as problems; no navbar rendering nothing, the comment-mode switch in the corner, and the theme selector off with `theme-selector: false` while the switch stays. A logo before the title and as the tab's icon, for a doc and a deck too; a missing logo as a problem and the page shown without it. |
| `nav` | 3 | | Links and `###` groups read from the Topics section only; the tree stopping at a cycle; a Topics entry outside the site or to a missing file is a problem. |
| `pdf` | 2 | 15 | The default file name made safe; a site refused. In Chrome: the logo loaded at 22px before the navbar's title, centred on it, and the tab's icon; the navbar's dropdown by click and keyboard, its highlight following an in-place link; the theme selector following the system until clicked, then switching every token and remembering the theme across a reload; the navbar folding into a menu on a narrow screen; the fixture doc on A4 with its header and footer; the devpage talk one 16:9 page per slide; a deck's keys; a doc's outline above the text on a narrow screen; every system widget rendering, long lines scrolling, task lists without bullets and an inline widget keeping its paragraph; widgets in a doc and a deck, a page loading only the widgets it uses; widgets mounting again after a live update and an edited widget or helper reaching open pages; a comment anchored inside a widget; a printed doc showing each widget's first render, terminal lines wrapping on paper; a broken helper's message and an unknown tag's error shown in place, and a project `Callout.tsx` leaving the system `Callout` rendering. |
| `render` | 11 | | Headings never taking an id the frame uses; every frame carrying the logo as its tab's icon, before the sidebar's and the outline's title; Raw HTML kept and Mermaid fences marked; highlighting without a language label; unknown languages left plain; tables and heading ids; a doc's chapters with its outline; a deck's slides; links kept for chapters and slides from other folders; a one-file doc's title; a page's title. |
| `server` | 12 | | Widgets served compiled, for the document they belong to, with versioned helper imports; a page naming its widgets and carrying their placeholders, code left alone; a site's page printing as a doc of one chapter; `/` and the site's pages following the entry file's changes without a restart, through an invalid state and back; comment threads over HTTP; deployed writes needing the header and stamped with it, over HTTP and in the identity rule alone; a thread on a page outside the served folder refused; serving a folder with no `.git` by what each file is; nothing outside the served folder served; a served folder reached through a symbolic link; each site's title linking to its own root page. |
| `watch` | 7 | | A saved page refreshing open pages; ignored files and files outside `watch` not refreshing; a comment thread always reaching its page; the `watch` list following the entry file; a site's `watch` narrowing only its own folder, whichever document devpage was started with; widget files refreshing whatever `watch` says. |
| `widgets` | 16 | | Widget tags expanded with Markdown content, expression, bare and self-closing props, code left alone — fenced, indented and inline code and HTML comments, but not indented list content; an unknown tag replaced by an error, not passed through; a `Tab` outside a `Tabs` reported at its page line; nested and inline widgets, an inline widget at the start of a line staying in its paragraph; `Example`'s highlighted source; TSX compiled with the automatic runtime, widget shims, `__devpageUses` and versioned relative imports; a compile error's line; compiles remembered until the source or version changes; a folder's version moving when a helper changes; lowercase files as helpers and a system widget's name as a problem. |

Tests that need files write them into folders under the system temp
directory, named with the test process's id, and don't all remove them.

**What has no test:** syntax highlighting except through the `render` tests;
Chromium's download when no Chrome is installed (it needs a machine without
one); fullscreen on a deck (headless Chrome refuses it without a real user
gesture); and how the light and dark themes look, which is checked by eye.

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

devpage is one binary — `main.rs` and eleven modules — with four built-in assets
and the system widgets.
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
  render --> widgets["widgets.rs<br/>widget tags, TSX"]
  render --> navbar["navbar.rs<br/>navbar, theme selector"]
  navbar --> icons["icons.rs<br/>Lucide, GitHub's mark"]
  server --> widgets
  server --> assets["assets/<br/>style, script, pele, React, icons, widgets"]
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
  click widgets href "../../viewer/widgets/README.md#under-the-hood" "Widgets under the hood"
  click navbar href "../../viewer/organizing-navigation/README.md#the-navbar" "The navbar"
  click icons href "../../viewer/vendored-icons/README.md" "The vendored icons"
  click assets href "../../viewer/serving-pages/README.md#built-in-assets" "Built-in assets"
```

| Module | Does |
| --- | --- |
| `src/main.rs` | The command line (clap): serving, `check` and `pdf`; wires the parts together and picks the address. |
| `src/document.rs` | `Document`: loads a document file of any kind, or a Markdown file as a one-chapter doc; the served folder and path safety; a site's `watch` patterns. `Entry`: the entry document, re-read when its file changes, keeping the last valid one while the file is invalid. |
| `src/index.rs` | The served folder's index of document files and of what `.gitignore` files exclude; finds a page's nearest site. |
| `src/watch.rs` | Watches the served folder, keeps the index current, and sends `change` or `comments /<page>` on the change channel. |
| `src/server.rs` | The routes, request logging, the event stream, the comment requests and identity, and the built-in assets. |
| `src/render.rs` | A page, a doc or a deck inside its frame; print mode; the problems bar; an invalid document file's page. |
| `src/nav.rs` | Reads Topics sections and builds a site's sidebar tree. |
| `src/highlight.rs` | Highlights fenced code for comrak, and marks Mermaid blocks. |
| `src/comments.rs` | The comment store: one YAML file per thread under `.devpage/comments/`, and the local author. |
| `src/check.rs` | Link checking, for the preview and `devpage check`, including external addresses. |
| `src/widgets.rs` | Widgets: the system and project widget sets and their problems; widget tags in Markdown turned into placeholders; TSX compiled by oxc, with helper imports versioned and compiled modules remembered. |
| `src/navbar.rs` | A site's navbar — its links, dropdowns and icons, which link holds the page shown, and its problems — and the light/dark switch with the head script that sets the theme before the page paints. |
| `src/icons.rs` | Icons by name, drawn as inline SVG: the vendored Lucide set and GitHub's mark. |
| `src/pdf.rs` | `devpage pdf`: serves the document internally and has Chrome print it, fetching a Chromium when none is installed. |

What each crate does is in [Dependencies](../dependencies/README.md).

### How main wires it together

`main` in `src/main.rs` parses the command line. `devpage check` loads the
document, checks it and exits with `0` or `1`; `devpage pdf` loads it and
prints it. Serving starts in this order; a failure at any step stops devpage
with an error before anything is served.

```mermaid
flowchart LR
  load["1. Load the document<br/>Document::load"] --> index["2. Build the index<br/>Index::build"]
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
   files; the watcher and the server share it.
3. **Create the broadcast channel** that carries `change` and
   `comments /<page>` messages, and hand it to the link checker, which sends
   `change` when a background check of an external address finds it broken.
4. **Start the watcher** on the served folder. `main` keeps it for the whole
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
