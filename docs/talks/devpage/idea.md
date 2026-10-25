<Columns kicker="The idea" title="The page in the browser is the file on disk.">

<FileTree>

- my-project/
  - docs/
    - devpage.yml — the site
    - README.md
    - design/
      - devpage.yml — a doc
  - src/

</FileTree>

<Terminal title="my-project">

```
$ devpage docs
devpage → http://localhost:3030
GET /docs/README.md → 200 in 1.4 ms
```

</Terminal>

</Columns>
