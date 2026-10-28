# Viewer glossary

The words the viewer owns, one section per term, so any page can link to a
term by its anchor — `glossary/README.md#served-folder`. The rest of the docs use
these words with these meanings. It is the reference page for the
[viewer](../README.md)'s vocabulary.

## document

What devpage serves and a reader opens: a site, a doc, slides or a design. Every kind is a document; the code uses the same word (`Document`, `Kind`).

## kind

Which of the four a document is. A site, a doc or slides is named by the `kind` field of its document file — `site`, `doc` or `slides`; a design is its own `.design` file and has no document file.

## site

A document of many pages with a sidebar built from Topics sections. Configured by `root`.

## doc

A document read top to bottom as one page — a design doc, a PRD — made of one or more chapters, with an outline beside it and PDF settings. The word for the kind is always *doc*, never "document".

## chapter

One Markdown file of a doc, listed in its `chapters` field.

## slides

A document that is a deck: one Markdown file per slide, in the order its `slides` field lists them, each laid out at 1920 × 1080. The kind is *slides*; one deck is a **deck**, one file a **slide**.

## notes

What is said over a slide: the text of the `<Notes>` tag in its file, read aloud by a presenter or a narrator and never shown on the slide. See [Speaker notes](../making-slides/README.md#speaker-notes).

## audience, goal, tone

A deck's fields for its narrator: who watches the talk, what it should achieve, and the energy of the whole talk. `audience` and `goal` are required by `devpage video`; `tone` has a default. See [What the narrator is told](../making-a-video/README.md#what-the-narrator-is-told).

## pronounce

A deck's list of lines telling the narrator how to say words a voice gets wrong, such as `Say "devpage" as "dev page".` See [What the narrator is told](../making-a-video/README.md#what-the-narrator-is-told).

## delivery

The `delivery` attribute of a slide's `<Notes>`: how that slide should feel when it is said — a direction for the narrator, never said or shown.

## narrator

GPT-Live, as `devpage video` uses it: told the deck's audience, goal and tone and each slide's delivery, it says each slide's notes word for word into the recording. See [The narrator](../making-a-video/README.md#the-narrator).

## document file

The YAML file describing a document — any name, `devpage.yml` by default.

## entry document

The document devpage was started with, where `/` leads.

## served folder

The folder devpage serves: the one it was started from, or the one `--root` names. Addresses are paths in it, `.devpage/` sits at its top, and nothing outside it is served, read or linked to.

## resolver

What decides the answer to a request from the documents alone — a page, a redirect, a file's bytes, a design's or a slide's PNG, or an error with its reason — so the HTTP server only turns that answer into a response (`Resolver`, in `core/src/resolve.rs`). See [Routes](../serving-pages/README.md#routes).

## mount prefix

The address every document a server serves sits under, carried by every redirect the resolver answers with; empty, since `devpage` serves at `/`.

## design

A UI drawn as code: one `.design` file holding one root `Frame`, drawn on a canvas and as a PNG. Never "mockup" or "component"; not to be confused with a design doc, which is a doc, or a design branch, which is a git branch.

## instance

A design used inside another, by the name it was imported as.

## prop

A value a design takes when it is used, declared with a default in its root's `props`.

## node

One element of a resolved design — a frame, text, rectangle, ellipse, vector or image; its **id** is where it came from — the [element ids](#element-id) from the design's root through each instance to its element — and survives inserts, removals and moves around it.

## scene

A design resolved into one tree of nodes, imports, props and theme colors substituted, every color with its light and its dark value, with the image files and font files its nodes use, ready to lay out and paint in either theme. A scene is **themed** when any `theme.` reference reached it.

## interaction

What clicking a node does in a prototype: `navigate`, `back`, `overlay` or `close`.

## prototype

Designs linked by interactions, clicked through on the canvas.

## design engine

The `devpage-design` crate: parses, resolves, lays out and paints designs, natively and in WebAssembly.

## drawing

What a design shows: its root's box, widened by anything painted outside it — a shadow, a stroke, a child placed past an edge — cut by any clipping frame and rounded out to whole pixels. A PNG is the drawing; the canvas page and an embed fit it. See [Viewing a design](../designs/README.md#viewing-a-design).

## element id

A number for each element of a file, unique in it and kept across edits: a new parse is matched against the last by `key`, then by a shortest edit script over each sibling list. See [Element ids](../design-engine/README.md#element-ids).

## key

An element's attribute naming it among its siblings, so it keeps its identity — and its node's id — however they are inserted, removed or reordered. See [Attributes](../designs/elements-and-attributes/README.md#attributes).

## layout boundary

A node whose own content can't change its size — a fixed width and height — so an edit inside it lays out only its subtree. See [Layout boundaries and runs](../design-engine/README.md#layout-boundaries-and-runs).

## run

An auto-layout frame whose children each take their own size, set one after another: each child laid out on its own, its place the prefix sum of the sizes before it, so a child's change of size moves no sibling. See [Layout boundaries and runs](../design-engine/README.md#layout-boundaries-and-runs).

## display list

What the painter makes of a laid-out scene in one theme: drawing commands in [chunks](#chunk), each command under its nodes in the [property trees](#property-trees), and the [index](#index) over them. See [Painting](../design-engine/README.md#painting).

## property trees

The transform, clip and effect trees a display list's commands refer to instead of nesting layers, so any subset of the list paints in order with exactly the layers it needs. See [Property trees](../design-engine/README.md#property-trees).

## chunk

One scene node's own commands, in its own coordinates; its position is a node in the transform tree, so a move keeps the chunk. See [Chunks](../design-engine/README.md#chunks).

## index

The two-level R-tree that answers what touches a rectangle: over the chunks in the design's coordinates, and per chunk over its commands. Painting, an edit and a click all ask it. See [The index](../design-engine/README.md#the-index).

## paint backend

What replays a display list in a rasterizer's calls — `vello_cpu` on the CPU, `vello` on the GPU — through the design engine's one port, `display::Target`: paint this area at this scale into this target. A backend translates each command and decides nothing. See [How the engine is built](../design-engine/README.md#how-the-engine-is-built).

## tile

A square of 256 device pixels of a drawing, painted at one scale in one theme and cached; [damage](#damage) makes the tiles it overlaps stale. See [Tiles](../design-engine/README.md#tiles).

## damage

What an update to a scene touched: each changed chunk's old and new bounds. Only the tiles that overlap it are painted again. See [Tiles](../design-engine/README.md#tiles).

## compositor

What draws a canvas from its tiles: every frame of a pan or zoom only moves and scales cached tiles, and missing or stale ones are painted after it and swapped in. See [The compositor](../design-engine/README.md#the-compositor).

## canvas page

The page a design's own address answers: one canvas filling the window, which the reader zooms and pans, with the light/dark switch in its corner; a navigation in the prototype changes the address. See [The canvas page](../serving-pages/README.md#the-canvas-page).

## embed

A design shown inside a page, written as an image of it — `![Checkout](checkout.design)` or `<img src="checkout.design">` — and drawn at its own size on a backdrop, scaled down only to fit; a navigation stays inside it. See [Viewing a design](../designs/README.md#viewing-a-design).

## backdrop

The quiet box an embed sits centred on, setting it apart from the text, with the link to its canvas page in its corner — and, for a themed design, the embed's own theme switch beside it. Never "stage", which is a slide's 1920 × 1080 box. See [Viewing a design](../designs/README.md#viewing-a-design).

## watch stream

A page's one gRPC-Web call, `Watch`, naming every design the page shows and carrying their updates — each whole scene first, then patches or problems — for as long as the page is open. See [The watch stream](../design-engine/README.md#the-watch-stream).

## patch

An update on the watch stream after a save: the nodes whose subtree changed since the scene last sent, each replaced by its id, and the image files and font files the scene last sent didn't have. See [The watch stream](../design-engine/README.md#the-watch-stream).

## image file

A PNG or JPEG an `Image` shows, known by its content — the SHA-256 of its bytes — so a scene holds, decodes and sends it once however many nodes show it. See [Resolving](../design-engine/README.md#resolving).

## font

What a `Text` is set in: Inter, which devpage carries inside it, when the `Text` has no `font`; or the font its `font` names — a **font name**, lowercase letters, digits and `-`, that a `themes.yml`'s `fonts` gives a font file or a folder of one family's font files. A design can mix several. Never a font installed on the machine, and never "typeface", which is a font's design rather than its files. See [Fonts](../designs/README.md#fonts).

## font file

A `.ttf` or `.otf` file of one font, at most 4 MB, in the served folder, that a design's text is set in; known by its content, like an image file, so a scene holds, checks and sends it once however many texts use it. See [Font files](../design-engine/README.md#font-files).

## page

A Markdown (`.md`) file in the served folder, rendered into a frame when requested. Any other file is served as it is.

## root page

The page a site's `root` names: where the site opens, and the top of its sidebar tree.

## page frame

The HTML a page is wrapped in: the stylesheet, the scripts and the kind's own layout — a site's sidebar, a doc's outline, a deck's stage.

## outline

A doc's navigation: its chapters' headings, beside the text, following the reader as they scroll.

## Topics

A page's `## Topics` section. The first link of each top-level list item names a child of the page in a site's sidebar.

## entry

One page in a sidebar tree, labelled with its Topics link's text.

## group

A run of entries under one `###` heading in a Topics section, shown under that heading in the sidebar.

## branch

An entry with children in the sidebar. Its chevron opens and closes it; the branch down to the page shown is open, and those the reader opened stay open, remembered per document. See [Organizing navigation](../organizing-navigation/README.md#opening-and-closing).

## search index

The full-text index of the sections of everything the served document publishes — the pages its nav reaches and the chapters and slides of the documents those reach, split at their headings — held in memory and kept current by the watcher. The code calls it `Search`. See [Search](../search/README.md).

## page result

One page a search found: its title, address, kind (`page`, `chapter` or `slide`), trail, document and lede, its best sections and a count of the rest. The code calls it `PageResult`. See [The endpoint](../search/README.md#the-endpoint).

## section result

One section of a page result: its heading, its path of headings, its address with the heading's anchor, a snippet of its prose with the matched words marked, and those words as its text spells them. The code calls it `SectionResult`.

## hit

One section result as the search box shows it: its page's title and trail, its heading and its snippet. See [The search box](../search/README.md#the-search-box).

## title match

A page whose title the query is — the same words but stopwords, the last perhaps still being typed. It comes first, and opens at its top. See [Ranking](../search/README.md#ranking).

## front matter

YAML between `---` lines at the very top of a page, never shown. Search reads `search`, `description` and `keywords` from it. See [Front matter](../configuration-reference/README.md#front-matter).

## search box

The field at the top of a site's sidebar that searches the search index as the reader types; while it holds text, its hits take the tree's place. `/` or ⌘K / Ctrl+K focuses it. See [The search box](../search/README.md#the-search-box).

## live update

An open page fetching itself again and swapping in its content and navigation, in place, after a watched file changed.

## change bus

What tells open pages there is something new to show: typed changes — a refresh, or the comments of a Markdown file — each published for a scope, by the watcher, the external-link checker and the server's comment writes (`Changes`). See [The bus and the stream](../live-updates/README.md#the-bus-and-the-stream).

## scope

Which open pages a change on the change bus is for: the documents under one mount prefix, or every document (`Scope`).

## out of sight

A path the index leaves out: inside a `.git` or `.devpage` folder, or excluded by a `.gitignore` in the served folder. Everything else is in sight. Nothing out of sight is watched, except the comment store and an ignored folder a site's `watch` pattern explicitly reaches into. See [What is out of sight](../live-updates/README.md#what-is-out-of-sight).

## watch set

The folders the watcher watches: the deep folders and the shallow folders of the served folder, planned again when a `.gitignore`, a site's `watch` list or a folder in sight changes. See [The watch set](../live-updates/README.md#the-watch-set).

## deep folder

A largest folder with nothing out of sight anywhere under it: watched recursively.

## shallow folder

A folder in sight that holds something out of sight — the served folder itself, usually: watched for its entries and the files directly in it.

## comment mode

The switch beside the theme toggle that turns commenting on: while it is on, selecting text or hovering a paragraph offers a comment. Off until the reader turns it on; the browser remembers it.

## comments off

devpage serving with `--no-comments`: its pages show no comment UI and every comment request answers `404`. For a public site. Comment mode is the reader's switch; comments off is the server's setting.

## comment

A thread a reader starts, in comment mode, on selected text or on a paragraph: the first message and its replies, one level deep. Stored in the comment store; resolving deletes it.

## anchor

What a comment is attached to: the quoted text, with a little of the text before and after it to tell repeats apart.

## detached comment

A comment whose quoted text is no longer on its page. It stays readable, listed at the top of the page, until it is resolved.

## comment store

Where threads are kept, behind the `Store` interface — list, create, reply, resolve. On disk it is `.devpage/comments/` at the top of the served folder: one YAML file per thread, which git never sees — `.devpage/` ignores itself. See [The comment store](../comments/the-comment-store/README.md).

## author

Who wrote a comment: `git config user.name` (or the system's user name) when devpage runs locally, the identity header's value when it is deployed.

## deployed

devpage listening on an address other than the local machine's (`--host`), behind an auth proxy that names the reader in a header.

## bundled

Deploying devpage with its documents built into the image: `devpage <path>` in a container, the local flow behind a proxy. Comments are files on a volume, or off. See [Deploying devpage](../deploying/README.md).

## server mode

`devpage server`: documents are published to it one at a time and kept with every version, in a store, a bucket and a cache. See [Server mode](../server/README.md).

## publish

Sending a document to a server as a zip, with `devpage publish`. The first publish creates the document and its id; each one after adds a version.

## version

One published state of a document in server mode, numbered from 1. Its files never change; `/d/<id>` shows the latest.

## context

A server the CLI talks to, named in `~/.devpage/config.yml` with an optional token command — as `kubectl` names a cluster.

## problem

A broken link, a missing chapter or slide, a Topics entry pointing nowhere, an invalid document file, a design that doesn't parse or resolve, or a diagram that doesn't parse — shown on the page in the preview and reported by `devpage check`.

## diagram

A fenced code block whose language is `mermaid`, drawn by the server as SVG in the page's HTML. See [Diagrams](../diagrams/README.md).

## diagram engine

pele running in QuickJS on a thread of its own in the server, with a cache of what it drew and limits on each drawing's time and memory (`core/src/diagrams.rs`). See [The diagram engine](../diagrams/README.md#the-diagram-engine).

## print mode

A page rendered for Chrome to print: no live updates, no comments, no navigation chrome. What `devpage pdf` captures.

## built-in asset

One of the files compiled into the binary and served under `/__devpage/`.

## content address

An address that changes exactly when the bytes it serves would: a built-in asset's carries the hash of its bytes (`/__devpage/devpage.<hash>.js`), a widget's its version (`?v=<hash>`). What answers one never changes, so the browser keeps it for good. See [Caching](../serving-pages/README.md#caching).

## load set

Everything a page loads from devpage beyond its stylesheet and page script — its widgets' modules, those they render by name, their helpers, React, the design script and engine — named in its `<head>` at their content addresses, so the browser asks for all of it at once. The code uses the same words (`Widgets::load_set`). See [The load set](../rendering-a-page/README.md#the-load-set).

## module cache

What devpage has read and compiled of the widget files it serves, with each one's version, held by the served folder's index and forgotten file by file as files change (`Modules`). See [How widgets run](../widgets/how-widgets-run/README.md).

## build id

A hash of everything devpage's binary is built from, on every page's `<body>`: a page from another build is loaded whole, never swapped in.

## widget

A React component used in Markdown as an HTML tag whose name starts with a capital letter — `<Callout>`. Its props come from the tag's attributes and its content, rendered as Markdown, is its `children`.

## system widget

A widget devpage ships, in every document: `Callout`, `Tabs`, `Tab`, `Example`, `FileTree`, `Terminal`, `Keys`, `Steps` and `Cards`.

## project widget

A widget a project writes: one `.tsx` file in the folder its document file's `widgets` field names, named by its file name.

## theme

A set of values for the color and font tokens. There are two, light and dark: by default the reader's system setting picks one, and the theme selector — one button — switches between them. A design has its own two, light and dark, from its `themes.yml`, and is drawn in the one of the same name as the page it is on. See [Themes](../designs/README.md#themes).

## themes.yml

The file a design's themes come from: `kind: themes`, then `light` and `dark`, each a map of names to colors that a design writes as `theme.<name>`. It may also hold `fonts`, a map of font names to font files — fonts are no part of a theme. A design uses the nearest one up the folder tree from its file; a lower one overrides only the names it lists, of colors and of fonts. Its colors are never called tokens, which are the page's. See [Themes](../designs/README.md#themes).

## navbar

A site's bar across the top: its title, then the links, dropdowns and icons its `navbar` field lists.

## token

A CSS custom property in `core/assets/style.css` — `--accent`, `--syntax-string` — that the page, its code and its diagrams take their colors and fonts from.
