# The native boundary is versioned

<Callout kind="tip" title="A review rule, written for the AI reviewer">

This file is a constraint for automated AI code review. On every change to this box or to anything inside it, the
reviewer reads it and checks the diff against the rule below; a change that
breaks it is a finding. People can read it too, but it is written to be
checked: the rule, why it holds, and how a breach shows.

</Callout>

<Callout kind="note" title="The rule">

Only `devpage_extension_v1`'s `VTable` and JSON bytes in `Buffer`s cross between devpage and a package. Devpage checks the manifest's `api`, `target`, `id` and library path, then the table's `version` and `size`, before it sends a request. Each side frees only buffers it allocated. No Rust type, trait object or panic crosses.

</Callout>

## Why

A package is built separately from devpage, and Rust's own ABI is not stable. Without a fixed table and buffer ownership, a package that loads can still corrupt memory on its first call.

## Check

`NativeLoader::load` in `src/adapters/extensions.rs` rejects a wrong API, target, ID format, path or table before returning a package. `Native::invoke` refuses a `create` under a key other than the manifest ID and releases every reply through the package's `release`. `extension-api/tests/abi.rs` checks replies, errors, malformed requests and a caught panic through the exported table. Nothing contains a native abort or memory fault: those end devpage.
