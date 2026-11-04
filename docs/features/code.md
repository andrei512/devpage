# Code reference

A site can have a page for every module, type and function of its Rust code,
read from the code as it is now: each page shows the declaration, the doc
comment and the members, in the site's frame. A save updates the pages, a
link from prose into the reference is checked against the code, search finds
it, and nothing is written to disk. A Markdown file headed
``` # ``rust:my_lib::Store`` ``` adds prose to that item's page.

```yaml
extensions:
  code:
    config:
      rust:
        files:
          - "../src/**/*.rs"
```

Teaches it: [Code reference](../boxes/viewer/code-reference/README.md).
