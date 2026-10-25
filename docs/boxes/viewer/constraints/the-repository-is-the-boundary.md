# The repository is the boundary

devpage serves, reads, watches and links only within the **repository** — the
git working tree holding the entry document, or the entry's folder when it is
in none. A request, a link, a chapter, a slide or a Topics entry that resolves
outside it — through `..` or through a symbolic link — is not part of any
document: a request for it gets `404`, and the link checker reports it as a
problem.

**Why:** a document is read by whoever can reach devpage, and once deployed
that is more than its author. A path that can climb out of the repository
serves whatever the process can read — keys, other projects, the home folder.
Inside one repository, by contrast, everything is already shared with anyone
who has the repository, so linking across documents in it costs nothing.

**Check:** every path that comes from outside the code — a URL, a link target,
a `root`, `chapters` or `slides` entry, a comment's page — is resolved with
symbolic links followed and compared against the repository's canonical path
before it is read. A test asks for `/../<file>` and for a symbolic link
pointing outside, and gets `404` for both.
