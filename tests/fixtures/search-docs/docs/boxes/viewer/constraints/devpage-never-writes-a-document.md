# devpage never writes a document

<Callout kind="note" title="The rule">

devpage never creates, changes or deletes a file a document is made of — a
Markdown file, a document file, an image, anything in the served folder outside
`.devpage/`. The server writes only inside `.devpage/` at the top of the served
folder: the comment store and the `.gitignore` that keeps git away from it. A
comment is never stored in the page it is about. The only other files devpage
writes are the PDF `devpage pdf` was asked for and the video `devpage video`
was asked for, each at the path it was given (or `<title>.pdf`,
`<title>.mp4` in the current folder); the Chrome each drives keeps a temporary
profile it deletes afterwards and, the first time, is downloaded —
chrome-headless-shell, or Chrome for Testing for a video — into a cache
outside the served folder.

</Callout>

<FileTree>

- my-project/ — the served folder
  - docs/ — never written by devpage
  - .devpage/ — the only folder the server writes
    - .gitignore — holds `*`
    - comments/ — one file per thread

</FileTree>

## Why

The documents are the repository's source of truth, written by people
and agents and reviewed in diffs. A tool that edits them behind the author's
back puts changes in a diff nobody made, and a comment written into a page
rides into a commit and onto main. Keeping every server write in one ignored
folder means resolving comments is deleting files, and `git status` never shows
the server's work. A PDF is different: it is the output someone asked for, by
name, with a command — and so is a video.

## Check

The server's filesystem writes all go through the comment store,
whose paths are built under `<served folder>/.devpage/`; `devpage pdf` and
`devpage video` write only their output file. A test runs a comment's whole life — create, reply,
resolve — and finds the repository's tracked files byte-for-byte unchanged and
`git status` clean.
