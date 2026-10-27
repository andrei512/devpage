---
id: "08"
blocked-by: ["01", "02", "03", "04", "05", "06", "07"]
boxes: [engineering/code-map, engineering/dependencies]
---

# Modules arranged by role, the core in its own crate

The code is arranged by role: the core (document, index, served folder,
markup, render, navigation, check, resolver, widgets' pure parts, the design
store) in its own crate with no tokio, axum, notify, tonic or chromiumoxide;
the ports it defines; the driven adapters (disk, YAML comments, watcher,
external HTTP checker); the driving adapters (HTTP, CLI, Chrome for PDF, PNG
and video). The compiler then refuses a core that reaches for I/O runtimes.
The code map shows the new layout.

## Acceptance

- The core crate builds with none of those dependencies.
- No behaviour change; `cargo install devpage` still installs one binary.
- The code map and dependency docs describe the new layout.
- Gates green including `cargo test -- --ignored`.
