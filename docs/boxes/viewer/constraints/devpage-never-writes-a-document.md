# devpage never writes a document

The only files devpage creates, changes or deletes are inside `.devpage/` at
the top of the repository — the comment store and the `.gitignore` that keeps
git away from it. It never writes a Markdown file, a document file, an image
or anything else a document is made of. A comment is never stored in the page
it is about.

**Why:** the documents are the repository's source of truth, written by people
and agents and reviewed in diffs. A tool that edits them behind the author's
back puts changes in a diff nobody made, and a comment written into a page
rides into a commit and onto main. Keeping every write in one ignored folder
means resolving comments is deleting files, and `git status` never shows
devpage's work.

**Check:** every filesystem write in the code goes through the comment store,
whose paths are built under `<repository>/.devpage/`. A test runs a comment's
whole life — create, reply, resolve — and finds the repository's tracked files
byte-for-byte unchanged and `git status` clean.
