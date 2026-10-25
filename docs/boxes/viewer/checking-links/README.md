# Checking links

How devpage finds broken links: what counts as a problem, how the preview shows
problems while you write, and how `devpage check` fails a test or a CI job on
them. It is a task article under the [viewer](../README.md).

## What is checked

Every link a reader could follow and every file a page loads:

- **Links and images** in every page — Markdown links, images, links and
  `src` attributes in raw HTML, and the targets of Mermaid `click` lines.
- **Targets inside the served folder** — the file must exist, inside the
  [served folder](../constraints/the-served-folder-is-the-boundary.md); a `#fragment`
  must name a heading or id on the page it points at; `#n` on a deck must be a
  slide it has.
- **Document files** — each must be valid for its kind, and its `root`,
  `chapters` and `slides` must name existing files.
- **Topics entries** — each must point at an existing page.
- **External links** — `http` and `https` addresses must answer with a status
  below 400, following redirects, within 10 seconds. devpage asks for each one
  as a browser asks for a page, accepting HTML, since some sites answer other
  requests with an error no reader sees. Each address is asked once per check,
  however many links point at it.
- **Comment threads** — each file in the comment store must parse.

`mailto:` and other schemes are not checked.

Links in a [widget](../widgets/README.md)'s content are checked like any other,
since devpage renders that Markdown. Links a widget's own code builds in the
browser are not: devpage never runs widget code on the server.

## In the preview

While devpage serves a document, each page lists its own problems in a bar at
its top, and every broken link on it is marked where it stands — a red wavy
underline on a link, a dashed outline on an image, a dashed red border on a
diagram node. They update as
you edit, like the rest of the page: fix the link, save, and the problem goes.
devpage also prints each problem once on its terminal, as it first finds it:

```text
docs/boxes/viewer/README.md:42: broken link ../serving/README.md — no such file
docs/README.md:7: broken link design/devpage.yml — in docs/design/devpage.yml: chapter rollout.md does not exist
docs/README.md:9: broken link talk/devpage.yml#9 — the deck has 3 slides
docs/README.md:12: https://example.com/gone — 404 Not Found
```

Files are named by their path from the top of the served folder. A document
file that can't be loaded is reported on the page that links to it.

External links are checked in the background, once per address for as long as
devpage runs, so a slow site never holds a page up; a problem with one shows up
on the page when its answer arrives.

## `devpage check`

```sh
devpage check docs            # the document in docs/devpage.yml
devpage check design.md       # one Markdown file
```

`devpage check` takes the same argument as `devpage`, checks that document and
every page and document reachable from it by links within the served folder,
prints every problem in the same `file:line: problem` form, and exits with `1`
if there was any and `0` if there were none. It starts no server. Use it in a
test or a CI job to keep the docs from shipping a broken link. devpage's own
tests check devpage's docs the same way, external links aside, so the test
suite needs no network.
