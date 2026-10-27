---
id: "01"
blocked-by: []
boxes: [viewer, viewer/serving-pages]
---

# One router builder and a pure request resolver

What a URL is — which document owns it, which page, design PNG, slide PNG,
canvas, raw bytes or redirect it answers with, which widgets apply — is decided
by a pure resolver that takes the document, the served folder, the URL path and
a mount prefix (empty today), and returns an answer. The HTTP adapter only maps
that answer to a response. The four router constructors collapse into one
builder taking one options value, so adding a dependency never again ripples
through every test router (the prototype's main cost).

Widget routes stop reloading every indexed document per request where the
index already knows the owner.

## Acceptance

- Every page route is tested through the resolver without a socket; the HTTP
  tests that only proved dispatch are replaced, not duplicated.
- One router builder; no positional-argument router constructors remain.
- A mount prefix exists and every address the resolver produces carries it;
  with the empty prefix every page is byte-identical to today.
- Gates green, including `cargo test -- --ignored`.
