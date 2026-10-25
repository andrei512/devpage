---
id: "11"
blocked-by: ["10", "12"]
boxes: [viewer/design-engine, viewer/serving-pages, engineering/dependencies, engineering/developing-devpage]
---

# The module is vendored and devpage publishes

As the [design engine](../boxes/viewer/design-engine/README.md#one-crate-two-targets)
page now says: the module and its bindings are committed under
`assets/vendor/`, one command rebuilds them and records a fingerprint of their
sources, a test fails when the fingerprint is stale and names the command, and
`devpage`'s build no longer builds WebAssembly. `devpage-design` carries what
it needs to build from the registry (its proto inside its package) and is
versioned with devpage.

## Acceptance

- `cargo package -p devpage-design` and `cargo package -p devpage` succeed,
  and `cargo install --path .` works without the wasm32 target installed.
- Changing one line of the engine makes the fingerprint test fail with the
  command's name; running the command makes it pass.
- Installing, serving pages, dependencies and developing pages describe it.
