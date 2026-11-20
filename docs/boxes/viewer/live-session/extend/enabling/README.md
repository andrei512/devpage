---
page-type: concept
---
# Where a part is enabled

A [world model part](../README.md#a-world-model-part) that does not come with
the live session ships in an extension package, and is enabled for one
project or for every project. This page says where each is written, which one
wins, and what is trusted.

## For one project

Under `extensions` in the project's `devpage.yml`, as any
[extension package](../../../extensions/reference/README.md#bundle) is
enabled:

```yaml
extensions:
  notes:
    package: ./notes
    config:
      limit: 50
```

`config` is the part's own; its functions read it as
[`now.config`](../part-reference/README.md#now).

## For every project

The same `extensions` mapping, in `~/.devpage/config.yml`, beside the
session's [settings](../../use/settings/README.md):

```yaml
extensions:
  notes:
    package: ~/parts/notes
```

## Which one wins

- **The project wins.** An entry with the same id replaces the one for every
  project
  ([extension configuration](../../../extensions/reference/README.md#configuration)).
- **`enabled: false` turns a part off for that project,** a part enabled for
  every project included.
- **The parts that come with the session are turned off the same way:**
  `devpage` and `canvas` are ids like any other.

  ```yaml
  extensions:
    canvas:
      enabled: false
  ```

A change takes effect when devpage restarts.

## What is trusted

- **A part runs as you.** Its module runs in the session's program, with the
  rights of the person who ran `devpage`. This is the trust a package's
  program has.
- **Only on your machine.** Packages enabled for every project are bound only
  while devpage serves on this machine, as the session is. A deployed devpage
  loads no part.
- **Never published.** `devpage publish` refuses a document with a bound
  package, so none is bound for `publish`, `check` or `pdf`.
