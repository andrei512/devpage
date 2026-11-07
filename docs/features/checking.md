# Checked links

Every link, image, heading anchor and diagram target is checked, and every
diagram must parse: broken ones are marked on the page while you write, and
`devpage check` fails on them.

<Terminal>

```
$ devpage check docs
docs/README.md:3: broken link design.md — no such file
docs/README.md:9: broken diagram — No diagram type detected for the given text.
```

</Terminal>

Teaches it: [Checking links](../boxes/viewer/checking-links/README.md).

For a reproducible build that must not depend on external sites, run
`devpage check docs --internal-only`. It still checks local links, anchors and
extension references; it skips only requests to external URLs. Normal `check`
continues to check those URLs too.

`devpage references docs --json` inspects the same source-owned catalog used
by generated pages, checking and search. Its `namespaces` object lists each
namespace's declarations, signatures, documentation and members, with source
paths relative to the served root. Invalid reference contracts fail the command.
Coverage tests can compare this catalog to a compiled public API without parsing
source files a second time.
