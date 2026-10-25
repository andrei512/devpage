# Comments

How readers comment on any document — a site, a doc or a deck — and where those
comments live: threads anchored to selected text or a paragraph, kept one file
per thread in a `.devpage/` folder git never sees, and resolved by deleting
that file. It is a concept and task article under the [viewer](../README.md),
with the store's format as its reference section for anyone — person or agent
— reading or resolving comments from the files.

Comments are working notes about the docs, not part of them. Locally, the loop
is: you comment while reading, tell your agent to check the comments, and it
changes the docs and deletes the threads it has dealt with. Deployed, a team
comments and replies in the browser.

## Commenting

- **On text** — select it, and a **Comment** button appears beside the
  selection.
- **On a paragraph** — hover over it, and a comment button appears in the
  margin. The whole paragraph — or list item, table, code block or diagram — is
  the anchor.

Write the comment and save it. It appears in the margin beside its text, which
is highlighted; on a narrow screen, and on slides, comments sit in a panel
instead (**C** toggles it on a deck). Every open page showing that text gets the
comment at once.

Each comment is a **thread**: the first message, then replies, one level deep
as in Google Docs. Anyone can reply. **Resolve** deletes the thread — there is
no archive and no "resolved" state, because the store is not history. Anyone
can resolve any thread.

Comments are never printed: print mode leaves them out
([A PDF is the page the browser shows](../constraints/a-pdf-is-the-page.md)).

## Who wrote it

A comment is stamped with its **author** and the time it was written.

- **Locally** the author is `git config user.name` as git sees it in the
  repository, or the system's user name when git has none.
- **Deployed** the author is the value of the identity header the auth proxy
  sets — `X-Forwarded-Email` unless `--user-header` names another. A comment
  without it is refused ([devpage authenticates no one](../constraints/devpage-authenticates-no-one.md),
  [Deploying devpage](../deploying/README.md)).

## When the text changes

A comment is anchored to the **rendered text** it quotes, with up to 32
characters before and after to tell repeats apart. When the page changes, the
comment follows its quote wherever it now is. When the quote is gone — the
sentence was rewritten — the comment is **detached**: it is listed at the top of
the page under *Detached comments*, still readable and resolvable, never lost
until someone resolves it.

## The comment store

Threads live in `.devpage/comments/` at the top of the repository — the same
folder locally and in a deployed container's volume. devpage creates
`.devpage/` with a `.gitignore` holding `*`, so git ignores the folder and
everything in it without the repository's own `.gitignore` changing. devpage
writes nothing else anywhere ([devpage never writes a document](../constraints/devpage-never-writes-a-document.md)).

One thread is one file, under the path of the Markdown file it is about:

```text
.devpage/comments/docs/design/rollout.md/20261025T181502Z-7f3a.yml
```

The file name is the thread's id: the UTC time it was started and four random
hex digits, so a folder lists its threads oldest first. A comment on a doc's
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
| `anchor.quote` | The rendered text the comment is on — the selection, or the whole paragraph's text. |
| `anchor.prefix`, `anchor.suffix` | Up to 32 characters of rendered text just before and after the quote; empty at a paragraph's edges. |
| `thread` | The messages, oldest first. The first is the comment; the rest are replies. |
| `author`, `at`, `text` | Who wrote a message, when (UTC, RFC 3339), and what it says. |

**Resolving from the files** is deleting the thread's file; an open page drops
the thread at once. A reply can be added by appending to `thread` — the page
picks it up too. A thread file devpage cannot parse is listed with the
page's problems ([Checking links](../checking-links/README.md)) and left alone.

## Under the hood

The page script asks for a page's threads when it loads, and writes through
four requests under `/__devpage/comments`: list a page's threads, start a
thread, reply, resolve. The comment store is watched like the rest of the
repository, so a thread written, changed or deleted — by a reader or by an
agent editing the files — reaches every open page through the
[live-update stream](../live-updates/README.md).
