# Working with an agent

What to tell a coding agent so it works with devpage the way you do: it
checks the links it wrote, looks at the designs and slides it drew, and deals
with the comments you left. It is a task page under the
[devpage landing page](README.md).

devpage has nothing to install for an agent. It reads the same files you
edit, and everything it shows is a file or an address the agent can fetch.
You keep `devpage` running; the agent edits, and your open pages update as it
saves.

## The block to paste

Put this in the repository's `AGENTS.md`, `CLAUDE.md` or the file your agent
reads, with your document's path and devpage's address in place of
`docs` and `http://localhost:3030`:

````markdown
## Docs

The docs are served by devpage (https://devpage.ai/docs/README.md) from
`docs/`, at http://localhost:3030 while I have it running.

- After changing docs, run `devpage check docs`. It prints each broken link
  as `<file>:<line>: <problem>` and exits 1 if there is one; fix them all.
- To see a design you drew, fetch its PNG:
  `curl -s http://localhost:3030/<path>.design.png -o design.png`
  (`?theme=dark` for the dark theme), then look at the image.
- To see a slide, fetch `http://localhost:3030/<deck file>/<n>.png`, counting
  slides from 1.
- My comments are files in `.devpage/comments/<page>/<thread>.yml`: the
  quote they are on and the messages. When asked to check the comments, read
  them, change the docs, and delete each thread file you have dealt with.
````

## What each part relies on

- **Checking links**: `devpage check` reads a document as it is shown and
  follows every link from it, external addresses too
  ([Checking links](boxes/viewer/checking-links/README.md)).
- **Design PNGs**: `GET <path>.design.png` draws the whole design
  ([Designs](boxes/viewer/designs/README.md)).
- **Slide PNGs**: every slide of a deck is a PNG
  ([Making slides](boxes/viewer/making-slides/README.md)).
- **Comments**: one file per thread, and deleting it resolves the thread
  ([The comment store](boxes/viewer/comments/the-comment-store/README.md)).
