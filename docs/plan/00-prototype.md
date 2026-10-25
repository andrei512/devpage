# Prototype report — design/v0.1

Measured: started 18:10:09, end-to-end path (doc → `devpage pdf` → PDF with
diagram, highlighting, header and footer) at 18:13:30 — about 3.5 minutes.
The rest of the budget went down the design: slides, the repository boundary,
linked documents, comments, deployed identity, link checking, the deploy
example, crate metadata and examples. Stopped at 18:29 with every part of
the design running once, end to end. The design update is not too big.

## What I touched

- **Document files** (`src/document.rs`): `kind`-tagged config (`site`, `doc`,
  `slides`) with `deny_unknown_fields`; `pdf` settings with defaults; a `.md`
  file as a one-chapter doc; the repository (nearest `.git`, else the folder)
  as boundary and URL root; `load_in` for linked documents; `site_for` (nearest
  site above a page). `watch` and `globset` are gone.
- **Rendering** (`src/render.rs`): the doc frame (chapters in one page, outline
  of `#`/`##` headings, chapter ids scoped as `<chapter>--<id>` with links
  between chapters rewritten), the slides frame (1920 × 1080 stage, one
  `section` per slide), print mode (`?print`: no nav, no live connection, no
  comments, `@page` size), `data-source` on every rendered section for
  comments, and a problems bar.
- **Serving** (`src/server.rs`): document files open as their kind, a page
  shows in its nearest site's frame or alone as a doc, comment routes
  (`GET/POST /__devpage/comments`, `…/reply`, `…/resolve`), `Identity`
  (local author, or the proxy header with `401` when it is missing).
- **New modules**: `src/pdf.rs` (chromiumoxide, `Page.printToPDF`, header and
  footer templates, Chromium fetch fallback), `src/comments.rs` (the store:
  one YAML per thread, self-ignoring `.devpage/`), `src/check.rs` (internal
  links, headings, images, diagram targets, linked documents, external links).
- **CLI** (`src/main.rs`): clap — `devpage [path] --host --port
  --user-header`, `devpage check [path]`, `devpage pdf <path> -o`; `--help`
  and `--version`.
- **Live updates** (`src/watch.rs`): the whole repository, skipping `.git`,
  `.devpage` and the root `.gitignore`; `comments <page>` messages for thread
  files.
- **Page script and stylesheet**: deck keys, scaling and `#n`; outline
  scroll-spy; comments panel, highlights, selection and paragraph buttons,
  detached list; print rules.
- **Repository files**: `LICENSE` (MIT), `deploy/Dockerfile`,
  `deploy/compose.yaml`, `deploy/Caddyfile`, `examples/sync-design`,
  `examples/launch-talk`, Cargo metadata, `docs/devpage.yml` with `kind: site`.

Tests: 21 run by `cargo test`, plus 2 `#[ignore]`d ones that drive the
installed Chrome (comments show and detach; deck keys). `cargo package`
lists the right files.

## Where the difficulty actually was

Not where expected. chromiumoxide printing worked on the first run, diagrams
included (the page marks itself ready after drawing). The real snags:

- **Chrome profiles collide.** chromiumoxide's default profile directory is
  shared, so two prints at once (two tests, or two `devpage pdf` runs) fail on
  Chrome's `SingletonLock`. Each launch now gets its own profile directory.
- **The example image didn't run.** `rust:1.89-slim` is built on a newer Debian
  than `debian:bookworm-slim`, so the binary needed `GLIBC_2.39`. The builder is
  now `rust:1.89-slim-bookworm`. Checked by building the same Dockerfile from
  local source (the crate isn't on crates.io yet) and running the compose file:
  behind the Caddy stand-in a comment is stamped `dev@example.com`, a request
  straight to devpage without the header gets `401`, and comments survive a
  container restart on the volume.
- **Diagram `click` targets** must be read from real Mermaid blocks in the
  rendered HTML, not from source lines — a Mermaid example inside a
  four-backtick `markdown` fence otherwise reports false broken links.
- **Several chapters on one page** share an id space; that needed id scoping
  and link rewriting, which the design states as an outcome but not a rule.

## What the design got wrong or left open

- **Compose port 8080** clashes with anything already on 8080 (Morph's backend
  container uses it on this machine). Not a contradiction — a choice the
  operator may want to revisit; the test used 8090.
- **The doc title above the first chapter** repeats the heading for a
  single-Markdown doc. The prototype shows it only for docs with a document
  file; the design doesn't say.
- **Chapter id scoping** (`<chapter>--<id>`) is visible in addresses. The design
  says ids stay unique and links land; it doesn't pin the form. A link from
  outside the doc to `chapter.md#heading` opens the chapter as a page of its
  nearest site (or alone), not scrolled within the doc.
- **The SDLC page links `https://crates.io/crates/devpage`**, which answers
  `404` until the first release, so `devpage check docs` fails on main until
  0.1.0 is published.
- **Design docs already have broken anchors** that `devpage check` found:
  `organizing-navigation` → `configuration-reference#root`,
  `developing-devpage` → `configuration-reference#the-document-file` and
  `serving-pages#choosing-a-port`; and `installing` → `../LICENSE` until the
  licence lands (it does on this branch now).
- **The `comments <page>` event** is per page; a doc or deck listens for each of
  its sources. Fine, but the live-updates page should say so.

## What is left to be production-quality

- **Document index.** The design describes an index of document files built at
  start and updated on change; the prototype scans folders on every request
  (`site_for`) and loads linked documents per request. Correct, not cached.
- **Nested `.gitignore` files** are not read by the watcher — only the root one.
- **External links in the preview** are not checked (only by `devpage check`);
  the design wants a background check cached per address, with results reaching
  open pages. External checks are not deduplicated by address either.
- **Problems of linked documents** (an invalid linked document file) appear in
  `devpage check`, not yet on the page that links to it; a Topics entry outside
  the site's folder is not yet a problem; Topics entries to document files are
  not yet sidebar entries.
- **Comments UI** uses `prompt()` for text; needs a real input in the margin,
  narrow-screen layout, and the paragraph quote uses `innerText` while matching
  uses text content (whitespace can differ in nested lists). Comment writes
  from a deck's panel and the `C` key are untested.
- **Doc on a narrow screen**: the outline doesn't move above the text yet.
- **Slides**: `Backspace`/touch not handled; fullscreen untested in a test;
  slide type scale is a first guess.
- **PDF**: Chromium download path untested (needs a machine without Chrome);
  temporary Chrome profile folders are not deleted after a run; `devpage pdf`
  default file name with characters illegal in file names is not sanitized.
- **`--host` printing**: shows the bound IP, not a hostname.
- **Tests to add**: server-level tests for routes (document as kind, nearest
  site, 404 outside the repository, 401 without the header), a check of
  devpage's own docs in `cargo test` (internal links only) once they are fixed,
  PDF page-count tests behind `#[ignore]` (Chrome).
- **Docs**: the mechanism pages (`serving-pages`, `live-updates`,
  `navigation-tree`), `developing-devpage`, `dependencies` (new crates:
  chromiumoxide, clap, reqwest, ignore, serde_json, futures; globset removed)
  and `diagrams-and-themes` still describe parts of the old code.
- **Release**: publish 0.1.0, then build `deploy/Dockerfile` as written and run
  the compose file against it.
