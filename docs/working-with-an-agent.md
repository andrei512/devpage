# Working with an agent

What to tell a coding agent so it works with devpage the way you do: it
checks the links it wrote, looks at the designs and slides it drew, and deals
with the comments you left. None of it needs a server running. It is a task page under the
[devpage landing page](README.md).

devpage has nothing to install for an agent. It reads the same files you
edit, `check`, `png` and the server commands answer in JSON with `--json`,
and its exit codes tell a
document's problems from a command that couldn't run. You keep `devpage`
running; the agent edits, and your open pages update as it saves.

## The block to paste

Put this in the repository's `AGENTS.md`, `CLAUDE.md` or the file your agent
reads, with your document's path in place of `docs`:

````markdown
## Docs

The docs are served by devpage (https://devpage.ai/docs/README.md) from
`docs/`, at http://localhost:3030 while I have it running.

- After changing docs, run `devpage check docs --json`. Exit 0: clean.
  Exit 1: `{"problems": [{"file", "line", "message"}]}`; fix them all.
  Exit 2: devpage couldn't run; the error is on stderr.
- To see a design you drew: `devpage png <path>.design` (add
  `--theme dark` for the dark theme), then look at the PNG it prints. A
  design with problems exits 1 and prints them instead.
- To see a slide: `devpage png <deck folder> --slide <n>`, counting from 1.
- My comments are files in `.devpage/comments/<page>/<thread>.yml`: the
  quote they are on and the messages. When asked to check the comments, read
  them, change the docs, and delete each thread file you have dealt with.
````

## What each part relies on

- **Checking links**: `devpage check` reads a document as it is shown and
  follows every link from it, external addresses too
  ([Checking links](boxes/viewer/checking-links/README.md)).
- **Exit codes and JSON**: the same three exit codes for every command, and
  one JSON shape for problems ([exit codes](boxes/viewer/configuration-reference/README.md#exit-codes)).
- **Design PNGs**: `devpage png` draws the picture a running devpage
  answers `<path>.design.png` with
  ([Designs](boxes/viewer/designs/README.md#looking-at-a-design-as-a-png)).
- **Slide PNGs**: every slide of a deck is a PNG
  ([Making slides](boxes/viewer/making-slides/README.md#a-slide-as-an-image)).
- **Comments**: one file per thread, and deleting it resolves the thread
  ([The comment store](boxes/viewer/comments/the-comment-store/README.md)).
