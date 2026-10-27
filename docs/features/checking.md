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
