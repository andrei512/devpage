# Reading a published document

What a reader gets from a document's link: the latest version, live as it
changes, earlier versions, comments that carry over from one version to the
next, and a PDF or PNG to download. It is a concept article under
[Server mode](../README.md).

## The link

`/d/<id>` always shows the **latest version**. When a new one is published,
every open page swaps it in at once — the same
[live update](../../live-updates/README.md) a saved file gives locally.

**Older versions** are listed in the version menu, at the page's bottom left,
and stay at
`/d/<id>/v/<n>`, under a banner that links to the latest. A version never
changes ([a version never changes](../constraints/a-version-never-changes.md)),
so a link to version 3 shows the same thing for as long as version 3 exists.

The server's front page lists nothing: a document is reached only through its
link.

## Comments

Readers comment as they do on any devpage —
[comment mode](../../comments/README.md), a quote or a paragraph, replies.
Comments belong to the document, not to a version, and follow the latest:
they are shown and taken on `/d/<id>/`, while an earlier version is shown as
it was.

- **On each publish**, every open thread re-anchors on the new version by its
  quote and the text around it.
- **When the quote is gone**, the thread becomes a
  [detached comment](../../glossary/README.md#detached-comment), listed at the
  top of the page until someone resolves it.
- **Resolving** deletes the thread, as it does locally.

An agent reads the same threads from the CLI:

<Terminal>

```text
$ devpage comments 6f1c2a9e-…
chapters/rollout.md
  20261025T181502Z-7f3a  on v3  "the client retries every write"
    …On reconnect, [the client retries every write] until the server acknowledges it.
    Andrei Puni   18:15  Every write, or only the ones still in the outbox?
    ana@…         18:40  Only the outbox. I'll fix the sentence.
  20261025T190344Z-0b91  detached in v4  "retries are capped at five"
```

</Terminal>

With `--json` each thread carries the
[comment store's](../../comments/the-comment-store/README.md) fields plus its
`id`, `page`, the `version` it was started on, `detached`, and `detached_in`,
the version its text left.

## Downloads

| Kind | Download |
| --- | --- |
| doc, slides | PDF |
| design | PNG |

From the download button beside the version menu, `devpage download`, or
`/d/<id>/v/<n>/download`. A download is rendered the way
[`devpage pdf`](../../pdf/README.md) prints — the page the browser shows —
once per version, then kept in the [cache](../storage/README.md#the-cache).
