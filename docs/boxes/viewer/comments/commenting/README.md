---
page-type: task
---
# Commenting on a page

How to comment on a page of a site, a doc or a deck, reply and resolve, and
how to turn comments off. It sits under [Comments](../README.md), which says
what a comment is and what happens to it.

## Commenting

Commenting is opt-in, so reading a page never sprouts buttons. **Turn comment
mode on** with the speech-bubble button beside the theme toggle — at the right
end of the navbar, or in the top-right corner of a page without one. It shows
pressed while on, and the browser remembers it for the next visit. Threads
already written, their highlights and the margin show either way.

<Steps>

1. Turn comment mode on
   The speech-bubble button beside the theme toggle shows pressed while on.

2. Pick what the comment is on
   **Text**: select it, and a **Comment** button appears under the selection.
   **A paragraph**: hover over it, and a speech-bubble icon appears just to its
   right (inside its right edge when the window has no room beside it). The
   whole paragraph — or heading, list item, table, code block or diagram — is
   the anchor.

3. Write it
   A box opens at the top of the comments: write the comment and press
   **Comment** (or <Keys>⌘ Enter</Keys> / <Keys>Ctrl Enter</Keys>); **Cancel**
   or <Keys>Esc</Keys> drops it.

</Steps>

The comment's
text is highlighted, and on a wide window its thread sits in the margin level
with it. On a narrow window, and on slides, threads sit in a panel instead,
opened with the **Comments (n)** button in the corner — or <Keys>C</Keys> on a deck.
Clicking highlighted text opens its thread. Every open page showing that text
gets the comment at once.

Each comment is a **thread**: the first message, then replies, one level deep
as in Google Docs. Anyone can reply, in the box under the thread. **Resolve** deletes the thread — there is
no archive and no "resolved" state, because the store is not history. Anyone
can resolve any thread.

Comments are never printed: print mode leaves them out
([A PDF must look the same as the page on screen](../../requirements/a-pdf-looks-like-the-page.md)).

## Turning comments off

Comments are for a team in a trusted place — your machine, or behind the auth
proxy that names each reader. A public site has neither, so serve it with
**`--no-comments`**:

```sh
devpage docs --no-comments
```

Then the pages show no comment-mode switch, no threads, no highlights and no
**Comments (n)** button, and <Keys>C</Keys> on a deck does nothing. Every
[comment request](../the-comment-store/README.md#the-requests) answers `404`, as if
it weren't there, so devpage never writes a thread — nor creates `.devpage/` for
one. Everything else — live updates, link problems, a slide's PNG — is
unchanged. It is a setting of the server, not of a document: the same docs can
be served with comments for the team and without them in public
([Deploying devpage](../../deploying/README.md#a-public-site)).
