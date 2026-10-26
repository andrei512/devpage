# Live updates

How a saved file reaches every open page: a file watcher over the served
folder skips what isn't watched, a broadcast channel fans each change out to one
server-sent event stream per open tab, and the page script fetches its page —
or, for a change in the comment store, the page's comments — again and swaps
it in place. It also covers why
there is one stream per tab; [the page script](../page-script/README.md) is
the browser's side. It is a
mechanism page under the [viewer](../README.md), for someone about to change
`src/watch.rs`, `src/server.rs` or `assets/devpage.js`.

```mermaid
sequenceDiagram
  participant Editor as Person or agent
  participant Disk as Markdown file
  participant Watcher as watcher (src/watch.rs)
  participant Channel as broadcast channel
  participant Server as server (src/server.rs)
  participant Page as devpage.js in the tab
  Editor->>Disk: save
  Disk-->>Watcher: file system event
  Watcher->>Watcher: out of sight? collect the burst
  Watcher->>Watcher: watched?
  Watcher->>Channel: send("change")
  Channel-->>Server: one message per open stream
  Server-->>Page: event stream: data change
  Page->>Server: GET the current page (no-store)
  Server-->>Page: fresh HTML
  Page->>Page: replace page, sidebar and navbar, mount widgets
```

## The watcher

When devpage starts, `watch::start` in `src/watch.rs` watches the whole
[served folder](../writing-a-document/README.md#the-served-folder), recursively,
with the platform's recommended `notify` watcher. Its work happens in two
steps:

1. **Sifting, as each event arrives** (`Listeners::sift`). Accesses (a read,
   not a change) and errors are dropped, and so is every path that can't
   matter, judged from the path alone with no file system call: a path with a
   `.git` component, a path under `.devpage/` other than the comment store,
   and a path [out of sight](#what-is-out-of-sight) by a `.gitignore` — unless
   the `watch` list of the nearest site holding it names it. So a build
   rewriting `target/` costs one lookup per path in memory, and no listener
   hears of it. The `.gitignore` rules are the index's, so a changed
   `.gitignore` takes effect on the next event, without a restart.
2. **Handling, a burst at a time** (`Listeners::changed`, on a thread of its
   own). The paths that survive go to a thread that waits for one, collects
   whatever else arrives within 50 ms (`BURST`), and handles each path of the
   burst once, deciding it like this:

```mermaid
flowchart TD
  path["A changed path,<br/>canonicalized"] --> thread{"A comment<br/>thread file?"}
  thread -->|"yes"| comments["Send comments /page"]
  thread -->|"no"| entry{"The entry<br/>document's file?"}
  entry -->|"yes"| reload["Re-read it,<br/>count a refresh"]
  entry -->|"no"| seen{"Out of sight, or<br/>not in its site's watch?"}
  seen -->|"no"| refresh["Count a refresh"]
  seen -->|"yes"| quiet["No refresh"]
  click comments href "../comments/the-comment-store/README.md#the-requests" "Comments under the hood"
  click seen href "#what-is-out-of-sight" "What is out of sight"
```

Before the decision, a path is canonicalized — falling back to the path as
reported, as for a deleted file — and skipped if it isn't inside the served
folder; only paths that survived sifting are. A thread file in the comment store
(`.devpage/comments/<page>/<id>.yml`) sends `comments /<page>`; the comment
store is watched whatever a site's `watch` says, and never refreshes pages
itself; nothing else under `.devpage/` is looked at. Every other path is first
handed to the design store — see [Designs](#designs) — and passed to the
[index](../document-index/README.md#the-index) (`Index::changed`), so a new,
edited or deleted document file or `.gitignore` is reflected at once. The
entry document's own file is re-read (`Entry::reload`) — so `/` and the
`watch` list follow the file — and refreshes whatever `watch` says.

### What is out of sight

A path doesn't refresh when the index finds it out of sight — inside a `.git`
or `.devpage` folder, or excluded by any `.gitignore` in the served folder,
nested ones included — or when the nearest site holding it has a `watch` list
that matches none of it (`refreshes` in `src/watch.rs`, then
`Document::watches`, patterns relative to that site's folder). A document file
always counts, and so does a widget or helper — a `.ts` or `.tsx` file
directly in the widgets folder of the entry document or of any indexed
document — and a path in no site with a `watch` list, whichever document
devpage was started with.

If any path counted, it sends one `change` message for the burst. Nothing in
this needs a `.git` folder to exist. A `.gitignore`d file a site's `watch`
names gets past sifting — so the index and the design store hear of it — but
still doesn't refresh.

A `change` message carries no file name: every open page refreshes, whichever
file changed. The events of one save, or of many files saved together, that
arrive within 50 ms of the first are one burst and one refresh; a save
reported across a longer stretch can still refresh more than once. Creating a
file and deleting a file both count as changes.
Document files are re-read as they change, so editing one needs no restart.
While the entry's file is invalid, the last valid version stays in use, and
`/` and the pages of a site whose file is invalid show why instead; when a
refreshed page comes back in a different frame — that problem page replaced
by the fixed site, or a document that changed kind — the page script loads it
whole rather than swapping its parts.

The watcher lives for the whole run. Dropping it stops the platform's watcher
and drops the event handler, which ends the burst thread.

On macOS, `notify` watches through one FSEvents stream on the served folder,
with no latency (`0.0`, no defer) and no way to leave a subtree out: a
non-recursive watch is filtered in `notify`'s callback, not in the stream. So
every change under the folder, `target/` included, still costs devpage one
callback; sifting is what keeps that callback cheap.

## The channel and the stream

`src/main.rs` creates a `tokio::sync::broadcast` channel with room for 16
messages. A send with no open pages fails, and the watcher ignores the
failure.

`GET /__devpage/events` (`events` in `src/server.rs`) subscribes to the channel
and answers with a server-sent event stream. Every item from the subscription
becomes one event whose data is the message — `change`, or `comments` and
the page's path — including the error a subscriber
gets when it falls more than 16 messages behind, so a slow page still hears
that something changed. The stream sends a keep-alive every 15 seconds.

## Designs

A design on its canvas, or embedded in a page, is kept current by its own
gRPC-Web watch stream rather than by refetching: the watcher hands every
changed path outside `.devpage/` that survives
[sifting](#the-watcher) to the server's store of resolved designs — whatever a
site's `watch` list says, since any file may be part of a design — and the store resolves again only the designs made from that
file and streams the nodes that changed
([Design engine](../design-engine/README.md#the-watch-stream)).

A design's file is also an ordinary watched file, so saving one refetches the
open pages like any other change — that is how a page embedding the design
gets its problems bar again. The refetch keeps the page's canvases
([The page script](../page-script/README.md#swapping-a-page-in-place)), so a
design is redrawn only by its stream, and only where its nodes changed. A site
whose `watch` list leaves out `.design` files and the images designs use still
has its canvases redrawn, but its problems bar waits for the next refetch.

## The page script

What the script in every open page does with a `change` or `comments`
message — and with links, the theme, the navbar and hidden tabs — is on
[The page script](../page-script/README.md).

## Why one stream per tab

The reason is stated in the header comment of `assets/devpage.js`: browsers
allow only six open connections per host over plain HTTP/1.1, and an open event
stream holds one of them for as long as it is open. So a tab keeps a single
stream however many pages it visits — links inside the document swap the page
in place instead of loading a new one — and a hidden tab gives its connection
back until it is shown.

## Failure modes

- **A file that isn't watched changes.** Nothing refreshes; the next request or
  link still shows the new content, since pages are read on every request.
- **devpage stops.** Open pages stop updating. The browser's `EventSource`
  reconnects on its own once devpage is back, but the page is not fetched again
  on reconnecting, so changes made in between show with the next change or
  navigation.
- **A burst of events.** Events within 50 ms of the first are handled
  together, each path once, with at most one `change`; events spread over a
  longer stretch make one burst, and at most one `change`, every 50 ms.
- **The watcher reports an error.** `notify` hands the callback in
  `src/watch.rs` either an event or an error, and an error is dropped
  silently by `Listeners::sift` — nothing is logged and
  devpage keeps serving. A watch that has broken this way shows only as pages
  that stop refreshing while requests still work; restart devpage to start a
  new watcher. Only a watcher that can't be started at all is reported, when
  devpage starts (see the
  [configuration reference](../configuration-reference/README.md#errors)).

How comments reach a page is on [Comments](../comments/the-comment-store/README.md#the-requests).

## Topics

- [The page script](../page-script/README.md) — swapping pages in place,
  mounting widgets, the stream, links and history, the theme, the navbar and
  hidden tabs.
