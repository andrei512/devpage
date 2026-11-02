# The native boundary is versioned

A native package is loaded through the declared API version and target. Its C-compatible function table and request schema agree with the manifest. Rust implementation layouts, trait objects, panics, and allocator ownership never cross an unspecified boundary.

Rust packages can be built separately from the installed host. Without a defined calling and ownership contract, a compatible-looking package version is insufficient to make those calls valid. The Rust SDK hides the bridge; it does not remove the bridge's obligations.

Reject an unsupported API, target, manifest ID, or entry point before creating an instance. Validate the returned function-table header. Exercise response release, ordinary error and caught-panic paths, instance disposal, and shutdown ordering. Do not claim that an SDK can contain a native abort or invalid-memory crash.
