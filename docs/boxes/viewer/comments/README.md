# Comments

How readers comment on a site, a doc or a deck — a design is discussed on the
page that embeds it — and where those comments live: threads anchored to selected text or a paragraph, kept one file
per thread in a `.devpage/` folder git never sees, and resolved by deleting
that file. It is a concept and task article under the [viewer](../README.md),
with the store's format as its reference section for anyone — person or agent
— reading or resolving comments from the files.

Comments are working notes about the docs, not part of them. Locally, the loop
is: you comment while reading, tell your agent to check the comments, and it
changes the docs and deletes the threads it has dealt with. Deployed, a team
comments and replies in the browser.

## Commenting

Commenting is opt-in, so reading a page never sprouts buttons. **Turn comment
mode on** with the speech-bubble button beside the theme toggle — at the right
end of the navbar, or in the top-right corner of a page without one. It shows
pressed while on, and the browser remembers it for the next visit. Threads
already written, their highlights and the margin show either way.

With comment mode on:

- **On text** — select it, and a **Comment** button appears under the
  selection.
- **On a paragraph** — hover over it, and a comment icon, a speech bubble,
  appears just to its right (inside its right edge when the window has no room
  beside it). The whole paragraph — or heading, list item, table, code block
  or diagram — is the anchor.

Either opens a box at the top of the comments: write the comment and press
**Comment** (or ⌘/Ctrl-Enter); **Cancel** or Escape drops it. The comment's
text is highlighted, and on a wide window its thread sits in the margin level
with it. On a narrow window, and on slides, threads sit in a panel instead,
opened with the **Comments (n)** button in the corner — or **C** on a deck.
Clicking highlighted text opens its thread. Every open page showing that text
gets the comment at once.

Each comment is a **thread**: the first message, then replies, one level deep
as in Google Docs. Anyone can reply, in the box under the thread. **Resolve** deletes the thread — there is
no archive and no "resolved" state, because the store is not history. Anyone
can resolve any thread.

Comments are never printed: print mode leaves them out
([A PDF is the page the browser shows](../constraints/a-pdf-is-the-page.md)).

## Who wrote it

A comment is stamped with its **author** and the time it was written.

- **Locally** the author is `git config user.name` as git sees it in the
  served folder, or the system's user name when git has none.
- **Deployed** the author is the value of the identity header the auth proxy
  sets — `X-Forwarded-Email` unless `--user-header` names another. A comment
  without it is refused ([devpage authenticates no one](../constraints/devpage-authenticates-no-one.md),
  [Deploying devpage](../deploying/README.md)).

## When the text changes

A comment is anchored to the **rendered text** it quotes — the page's text as
the reader sees it, every run of spaces and line breaks counted as one space —
with up to 32 characters before and after to tell repeats apart. When the page changes, the
comment follows its quote wherever it now is. When the quote is gone — the
sentence was rewritten — the comment is **detached**: it is listed at the top of
the page under *Detached comments*, still readable and resolvable, never lost
until someone resolves it.

## The comment store

Threads live in `.devpage/comments/` at the top of the served folder — the same
folder locally and in a deployed container's volume. devpage creates
`.devpage/` with a `.gitignore` holding `*`, so git ignores the folder and
everything in it without any other `.gitignore` changing. The server
writes nothing else anywhere ([devpage never writes a document](../constraints/devpage-never-writes-a-document.md)).

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
page's problems ([Checking links](../checking-links/README.md)) and left alone.

## Under the hood

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
gets `401`; reading needs none. The comment store is always watched, whatever a site's `watch`
says, so a thread written, changed or deleted — by a reader or by an
agent editing the files — reaches every open page through the
[live-update stream](../live-updates/README.md).
