# How server mode works

The mental model of server mode: four ideas — a document, its versions, a
publish and a context — and one document followed from its first publish to
its second version. It is the essentials page of [Server mode](../README.md);
read it before the task pages.

## Four ideas

- **A document** is one of devpage's [kinds](../../glossary/README.md#kind) — a
  site, a doc, slides or a design — published on its own. The first publish
  gives it an **id**, a UUID, and a link: `/d/<id>`.
- **A version** is one publish of it, numbered from 1. A version never changes;
  the link always shows the latest.
- **A publish** sends the document's files to the server as a zip. Nothing
  broken gets in: the CLI and the server both check it first.
- **A context** is a server the CLI talks to, as `kubectl` talks to a cluster.
  The CLI remembers each document's id per context, so publishing the same
  file again updates the same document.

How the two deploy modes differ, side by side:

| | Bundled | Server mode |
| --- | --- | --- |
| Command | `devpage /srv/docs` | `devpage server` |
| Documents arrive | in the image | one publish at a time |
| Documents link to each other | yes | no — each stands alone |
| Versions | the image's | every publish |
| Comments | files on a volume, or off | in the store |

## One document's life

<Steps>

1. Publish it
   The server checks it, keeps it as version 1, and answers with its id and
   link. Every later command names the document by that id.

   $ devpage publish launch/devpage.yml

2. Readers open the link and comment
   On `/d/<id>`, exactly as on a local devpage.

3. The author's agent reads the comments
   Every open thread, with the quote it is on.

   $ devpage comments 6f1c2a9e-…

4. The agent edits the source and publishes again
   Version 2 replaces version 1 on every open page at once. Each comment
   re-anchors on the new text, or is listed as detached if its text is gone.

   $ devpage publish launch/devpage.yml

5. The agent resolves what it fixed
   $ devpage comments resolve 6f1c2a9e-… 20261025T181502Z-7f3a

</Steps>

Version 1 stays readable at `/d/<id>/v/1`.

## Where to go next

- [Publishing a document](../publishing/README.md) — connecting and publishing.
- [Reading a published document](../reading-a-published-document/README.md) —
  what readers see.
- [Running a server](../running-a-server/README.md) — deploying one.
