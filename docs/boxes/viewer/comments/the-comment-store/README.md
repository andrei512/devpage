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
page, each chapter of a doc, each slide of a deck — when it loads, and writes
through four requests:

| Request | Does |
| --- | --- |
| `GET /__devpage/comments?page=/docs/a.md` | The file's threads, oldest first, each with its `id`. |
| `POST /__devpage/comments` with `{page, anchor, text}` | Starts a thread; answers `{id}`. The page must be a file in the served folder. |
| `POST /__devpage/comments/reply` with `{page, id, text}` | Appends a reply. |
| `POST /__devpage/comments/resolve` with `{page, id}` | Deletes the thread's file. |

The three writes need an author: deployed, a write without the identity header
gets `401`; reading needs none. With comments off (`--no-comments`) all four
answer `404` and write nothing ([Turning comments off](../README.md#turning-comments-off)). The comment store is always watched, whatever a site's `watch`
says, so a thread written, changed or deleted — by a reader or by an
agent editing the files — reaches every open page through the
[live-update stream](../../live-updates/README.md).
