<Columns kicker="Checked" title="Broken links fail the build — not the reader.">

<Terminal title="CI">

```
$ devpage check docs
docs/README.md:12: broken link design.md — no such file
docs/README.md:30: broken link talk/devpage.yml#9 — the deck has 6 slides
$ echo $?
1
```

</Terminal>

</Columns>
