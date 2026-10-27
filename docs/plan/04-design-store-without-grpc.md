---
id: "04"
blocked-by: []
boxes: [viewer/designs, viewer/design-engine]
---

# The design store without its transports

The design store keeps the designs, their problems and the `Files` port it
already uses well. The gRPC-Web watch service and the canvas page move out of
it to the HTTP side, as driving adapters of the store.

## Acceptance

- The design store's module depends on no tonic or axum.
- The watch stream and the canvas behave as today; their tests pass unchanged
  in what they assert.
- Gates green including `cargo test -- --ignored`.
