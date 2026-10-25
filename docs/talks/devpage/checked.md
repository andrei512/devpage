<Columns kicker="Checked links" title="Broken links fail the build — not the reader.">

<Terminal title="my-project">

```
$ devpage check docs
docs/README.md:3: broken link design.md — no such file
docs/README.md:3: broken link talk/devpage.yml#9 — the deck has 3 slides
$ echo $?
1
```

</Terminal>

</Columns>
