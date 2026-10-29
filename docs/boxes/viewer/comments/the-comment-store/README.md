# The comment store

Where threads are kept and how to read or resolve them from the files — the
reference for anyone, person or agent, working with comments outside the
browser — and the requests the page script makes. It is a reference and
mechanism page under [Comments](../README.md).

## The files

Threads live in `.devpage/comments/` at the top of the served folder — the same
folder locally and in a deployed container's volume. devpage creates
`.devpage/` with a `.gitignore` holding `*`, so git ignores the folder and
everything in it without any other `.gitignore` changing. The server
writes nothing else anywhere ([devpage never writes a document](../../constraints/devpage-never-writes-a-document.md)).

One thread is one file, under the path of the Markdown file it is about:

<FileTree>

- .devpage/
  - .gitignore — holds `*`, so git never sees the folder
  - comments/
    - docs/design/rollout.md/ — the threads on that page
      - 20261025T181502Z-7f3a.yml — one thread
      - 20261025T190344Z-0b91.yml — another

</FileTree>

The file name is the thread's id: the UTC time it was started, to the second,
and four hex digits from the clock's nanoseconds that tell apart threads
started in the same second, so a folder lists its threads oldest first. A comment on a doc's
chapter or a deck's slide belongs to that chapter's or slide's file, so it
follows the file into any document that uses it.

```yaml
anchor:
  quote: the client retries every write
  prefix: "On reconnect, "
  suffix: " until the server acknowledges it."
thread:
  - author: Andrei Puni
    at: 2026-10-25T18:15:02Z
    text: Every write, or only the ones still in the outbox?
  - author: ana@example.com
    at: 2026-10-25T18:40:11Z
    text: Only the outbox. I'll fix the sentence.
```

| Field | Meaning |
| --- | --- |
| `anchor.quote` | The rendered text the comment is on — the selection, or the whole paragraph's text — with whitespace collapsed. |
| `anchor.prefix`, `anchor.suffix` | Up to 32 characters of the page's rendered text just before and after the quote, headings included; empty at the start or end of the page. |
| `thread` | The messages, oldest first. The first is the comment; the rest are replies. |
| `author`, `at`, `text` | Who wrote a message, when (UTC, RFC 3339), and what it says. |

**Resolving from the files** is deleting the thread's file; an open page drops
the thread at once. A reply can be added by appending to `thread` — the page
picks it up too. A thread file devpage cannot parse is listed with the
page's problems ([Checking links](../../checking-links/README.md)) and left alone.

## The requests

The page script asks for the threads of every Markdown file it shows — a
page, each chapter of a doc, each slide of a deck, never a widget — all at
once, when it loads, and writes through four requests:

| Request | Does |
| --- | --- |
| `GET /__devpage/comments?page=/docs/a.md` | The file's threads, oldest first, each with its `id`. |
| `POST /__devpage/comments` with `{page, anchor, text}` | Starts a thread; answers `{id}`. The page must be a file in the served folder — any file, linked from a document or not — or the request gets `422`. |
| `POST /__devpage/comments/reply` with `{id, text}` | Appends a reply to the thread with that id, wherever it is. |
| `POST /__devpage/comments/resolve` with `{id}` | Deletes the thread's file. |

The three writes need an author: deployed, a write without the identity header,
or with it blank, gets `401`; reading needs none. With comments off (`--no-comments`) all four
answer `404` and write nothing ([Turning comments off](../README.md#turning-comments-off)). Each write
announces that page's comments on the
[live-update stream](../../live-updates/README.md) as it lands, so every open
page fetches them again. The comment store is also always watched, whatever a
site's `watch` says, so a thread written, changed or deleted by an agent
editing the files reaches every open page too; a file the server just wrote is
not announced a second time.

## How a write goes

Every thread write — a page's request, a [server's](../../server/README.md)
API call, a publish carrying threads onto a new version — goes through one
module, `comments` (`core/src/comments/`), and its `Comments` value for the
served or version folder. It holds every rule of a write:

- **The page key.** A page is keyed by its path with one leading `/`
  (`/docs/a.md`); one that is empty or climbs out of the folder is refused.
- **Starting a thread.** The page must be a file in the served folder — or,
  on a server, in the version the thread is started on — resolved through the
  [served folder](../../constraints/the-served-folder-is-the-boundary.md).
- **Replying and resolving** go by the thread's id alone, with no page check,
  so a [detached comment](../../glossary/README.md#detached-comment) takes
  replies like any other.
- **The author** comes from one `Identity`: the local author, or the identity
  header's trimmed value, refused when missing or blank.
- **Announcing.** Each write that lands announces its page's comments once;
  a refused write announces nothing.
- **Carrying** a thread onto a new version reads each page as the page shows
  it — front matter left out, widget tags expanded — through the served
  folder. `Comments` works out where each thread goes and writes nothing: the
  publish records the version and those moves in one transaction, so a carry
  that fails publishes nothing, and only then are the moved pages announced.

Where threads are kept is the comment store's only job, behind the `Store`
port, by thread id: the YAML files above locally (`comments::yaml`, which finds
`<id>.yml` through the served folder), server mode's SQLite file
(`DocumentThreads`), and memory in tests. The store also says which pages'
threads a hand other than its own changed (`heard`), which is how the watcher
announces a hand edit and never the server's own write.
