---
id: "04"
blocked-by: ["01", "02", "03"]
boxes: [engineering/dependencies, engineering/developing-devpage, viewer/configuration-reference]
---

# The reference pages describe the workspace that was built

- Dependencies: every new crate with its locked version, licence, role and
  where it is used; the build scripts and what they produce; Inter's licence.
- Developing devpage: the workspace, the WebAssembly build inside
  `cargo build`, what needs a rebuild after changing the engine or the canvas
  script, and the new tests.
- Configuration reference: `.design` as the argument to `devpage` and
  `devpage check`, and the design errors.
- Installing: what the first build downloads and roughly how big the binary is.

## Acceptance

- `cargo test`'s docs link check is green and every statement on these pages
  is true of the code.
