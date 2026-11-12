---
page-type: mechanism
---
# How a comment is written

How a thread write reaches its file and every open page. It sits under [Comments](../README.md), for someone about to change how comments
are stored; the files and the requests are on
[The comment store](../the-comment-store/README.md).

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
