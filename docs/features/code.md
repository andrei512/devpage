# Code reference

A site can have a page for every module, type and function of its Rust, Python or TypeScript code,
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

The [Python example's `Graph`](../reference/python/sample/graph/Graph) is read
from its docstrings. PyO3 reads Rust binding attributes and registrations while
showing Python call signatures. Selected simple Rust macros can expand through
the same grammar. Native semantic providers contribute domain-specific nodes,
RPCs or schemas into these same checked pages, without a separate renderer.

Strict namespaces fail for missing abstracts, broken syntax, empty input and
unhandled file-scope Rust macros. Exclusion globs keep generated files and tests
outside the intended public surface; compiled API tests can compare against the
native inventory when runtime exports are the authority.

Teaches it: [Code reference](../boxes/viewer/code-reference/README.md).
