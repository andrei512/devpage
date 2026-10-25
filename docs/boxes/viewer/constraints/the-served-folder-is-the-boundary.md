# The served folder is the boundary

devpage serves, reads, watches and links only within the **served folder** —
the folder it was started from, or the one `--root` names. A request, a link, a
chapter, a slide or a Topics entry that resolves outside it — through `..` or
through a symbolic link — is not part of any document: a request for it gets
`404`, and the link checker reports it as a problem. Nothing about the boundary
depends on git: devpage never looks for a `.git` folder.

**Why:** a document is read by whoever can reach devpage, and once deployed
that is more than its author. A path that can climb out of the served folder
serves whatever the process can read — keys, other projects, the home folder.
Inside the folder, by contrast, the author chose what to share by choosing the
folder, so linking across documents in it costs nothing. Tying the boundary to
git would break it wherever there is no `.git` folder — in a container, an
export, a tarball.

**Check:** every path that comes from outside the code — a URL, a link target,
a `root`, `chapters` or `slides` entry, a comment's page — is resolved with
symbolic links followed and compared against the served folder's canonical
path before it is read. A test asks for `/../<file>` and for a symbolic link
pointing outside, and gets `404` for both; no code opens or searches for
`.git`.
