---
page-type: constraint
id: devpage-stays-inside-its-folder
---
# devpage must never serve or read a file outside the folder it was started on

<Callout kind="note" title="The rule">

devpage serves, reads, watches and links only within the **served folder** —
the folder it was started from, or the one `--root` names. A request, a link, a
chapter, a slide or a Topics entry that resolves outside it — through `..` or
through a symbolic link — is not part of any document: a request for it gets
`404`, and the link checker reports it as a problem. Nor is it ever named: a
path outside has no address and no name, so no page, problem or label shows
it. Nothing about the boundary depends on git: devpage never looks for a
`.git` folder.

In [server mode](../share/server/README.md) each version's served folder is the root
of its zip: an entry with `..`, an absolute path or a symbolic link refuses the
publish, and no document reads another's files.

</Callout>

## Why

A document is read by whoever can reach devpage, and once deployed
that is more than its author. A path that can climb out of the served folder
serves whatever the process can read — keys, other projects, the home folder.
Inside the folder, by contrast, the author chose what to share by choosing the
folder, so linking across documents in it costs nothing. Tying the boundary to
git would break it wherever there is no `.git` folder — in a container, an
export, a tarball.

## Check

Every path that comes from outside the code — a URL, a link target,
a `root`, `chapters` or `slides` entry, a comment's page, a thread file found
by its id — is resolved with
symbolic links followed and compared against the served folder's canonical
path before it is read, and every such comparison is made by one owner, the
served-folder value (`core/src/folder/served.rs`). Its `name` and `url` refuse
a path outside — `None` — rather than stand its absolute path in for one, and
each caller handles the refusal. They are the only way a path is named, and
nothing falls back to the path it was given when it can't name one — not even
where the path lies inside by construction. A test asks for `/../<file>` and for a symbolic link
pointing outside, and gets `404` for both; a page linking out never carries the
file's path; no code opens or searches for `.git`.
