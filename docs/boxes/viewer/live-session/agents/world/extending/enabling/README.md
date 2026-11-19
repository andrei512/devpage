---
page-type: concept
---
# Where an extension is enabled

How a [world part](../README.md#a-world-part) that is not the live session's
own reaches a session: it ships in an extension package, enabled for one
project or for every project.

- **A world part ships in an ordinary
  [extension package](../../../../../extensions/reference/README.md#bundle).**
  Its manifest names a JavaScript module, which the session's program loads;
  the module registers the part.
- **For one project:** `extensions` in the project's `devpage.yml`, as every
  package is enabled today.
- **For every project:** the same `extensions` mapping in
  `~/.devpage/config.yml`, beside the session's
  [settings](../../../../README.md#settings).
- **A project wins.** An entry with the same id replaces the global one, and
  `enabled: false` turns it off for that project
  ([extension configuration](../../../../../extensions/reference/README.md#configuration)).
- **Global packages are bound only while devpage serves on this machine,** as
  the session is. `devpage publish` refuses a document with a bound package,
  so they are never bound for `publish`, `check` or `pdf`.
- **Same trust as a package's program.** A world module runs as the person who
  ran `devpage`. A deployed devpage loads none.
