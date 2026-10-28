# Server mode

`devpage server` is a place to publish documents that have no repository to
live in. `devpage publish` checks a document, sends exactly the files it is
made of, and prints its link. The link always shows the latest version, live;
every earlier version stays readable at its own. Comments carry from one
version to the next, and docs and decks download as PDF, designs as PNG.

<Terminal>

```
$ devpage context add work https://docs.company.com
added context work
$ devpage context use work
using context work
$ devpage publish launch/devpage.yml
Launch — version 1
https://docs.company.com/d/6f1c2a9e-…/
$ devpage comments 6f1c2a9e-…
```

</Terminal>

It runs as one container beside a bucket — S3 or Azure Blob holds every
version, and the volume is only a cache — with your own proxy in front.

Teaches it: [Server mode](../boxes/viewer/server/README.md).
