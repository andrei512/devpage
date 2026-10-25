---
id: "05"
blocked-by: ["01", "02", "03", "04"]
boxes: [engineering, viewer]
---

# Engineering docs and release readiness

- Developing devpage (modules, startup, tests), dependencies (every crate with
  its locked version, licence and use; `globset` back only if `watch` uses it),
  diagrams-and-themes and every remaining page describe the code as built.
- `cargo package` contains what `cargo install` needs and nothing else; the
  example image builds from the packaged source and the compose file works
  behind the stand-in proxy (from local source, since 0.1.0 isn't published).
- Morph-independent: nothing in this repository refers to another project.

## Acceptance criteria

- `devpage check docs` passes (internal links); the docs-check test passes.
- Gates green; `cargo package --list` reviewed.
- Compose run: a comment is stamped `dev@example.com`; `401` without the
  header; comments survive a restart.
