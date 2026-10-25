---
id: "01"
blocked-by: []
boxes: [viewer, engineering]
---

# Widgets, production-quality

Harden the prototype into the design as amended:

- A page loads only the widgets it uses (and what they compose); compiled
  modules are cached and recompiled on change; helper addresses carry a version
  so an edited helper reaches open pages.
- Widget and helper files refresh open pages whatever a site's `watch` says;
  drop the `widgets/*.tsx` workaround from `docs/devpage.yml`.
- Lowercase files are helpers, capitalized files are widgets; `html`, `source`
  and `children` reach every widget.
- Tests: route and check tests per problem kind (unknown tag, clash, compile
  error), Chrome tests for each system widget's rendering, for widgets in a doc,
  a deck, a printed PDF, a comment anchored inside widget content, and for a
  live widget edit.
- Docs true to the code: the widgets page (as built), a page for the vendored
  React bundle (version, source, licence, how to upgrade — like the pele page),
  dependencies (oxc and the rest), developing-devpage (modules, tests), the
  serving-pages and live-updates mechanisms.

## Acceptance criteria

- Every bullet above has a test or a by-hand check in the report.
- Gates green, `cargo test -- --ignored` green, `devpage check docs` exit 0.
