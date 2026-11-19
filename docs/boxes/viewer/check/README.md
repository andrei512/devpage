---
page-type: landing
---
# Check

What `devpage check` holds a document to, in the preview and in a test: every
link and diagram, the page types a site defines and which may sit under which,
and requirements tied to the tests that prove them. It is a part of the
[viewer](../README.md).

```mermaid
flowchart LR
  top["Check"] --> n0["Structuring a site"]
  top["Check"] --> n1["Checking requirements"]
  top["Check"] --> n2["Checking links"]
  click n0 href "./structuring-a-site/README.md" "Structuring a site"
  click n1 href "./checking-requirements/README.md" "Checking requirements"
  click n2 href "./checking-links/README.md" "Checking links"
```

## Topics

- [Structuring a site](structuring-a-site/README.md) — a site's own page types,
  the type each page names, and which types may sit under which.
- [Checking requirements](checking-requirements/README.md) — a system's
  requirements as pages, tests that point at its constraints, and what
  `devpage check` reports from their results.
- [Checking links](checking-links/README.md) — what is checked, problems in the
  preview, and `devpage check` in tests.
