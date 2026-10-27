# Prototype: make rendering pure (stage 1, step 1)

Spike of the first step of the hexagonal refactor: rendering checks nothing
itself, the check ↔ render and document ↔ render/widgets cycles are gone, and
external links are asked through an injected port. Behaviour unchanged.

**Measured:** started 13:44:55, end-to-end path green at 13:52:53 (8 min),
all gates including the Chrome tests green at 13:56:12, docs and report by
about 13:58. About 13 minutes of a 30-minute budget.

**End to end:** a page request reaches the server, which asks its `Preview`
for the page's problems; the preview walks internal links purely and asks
external addresses through `External`, whose `Ask` port is `Http` in devpage
and a scripted table in tests; the problems are handed to `render::page`,
`render::doc` or `render::slides`, which only lay them out.

Gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo test` (223 passed) and `cargo test -- --ignored` (80 passed, Chrome).

## What it touched

- **New leaf modules.** `src/problem.rs` (`Problem`), `src/markup.rs` (front
  matter, `title`, `<Notes>` and the widget-tag scanner moved out of
  `render.rs` and `widgets.rs`), `src/external.rs` (the external-link checker
  and its `Ask` port).
- **`check.rs`.** The globals `CLIENT`, `EXTERNAL`, `CHANGES`, `REPORTED`, the
  wait atomics and `set_waits`/`announce_on` are gone. `check::document` takes
  `Option<&External>`; the preview is a `Preview` value the server owns.
- **`render.rs`.** `page`, `doc` and `slides` take the page's problems as an
  argument; render no longer imports `check`.
- **`server.rs`, `main.rs`.** `AppState` holds an `Arc<Preview>`;
  `router_with_index` and `quiet` take it; `show` asks for problems only when
  not printing, as before.
- **Production dependencies now:** `problem` and `markup` depend on nothing in
  devpage; `document` depends only on `markup`; `render` no longer depends on
  `check`; `widgets` no longer depends on `render`.
- **Tests.** Three new tests at the `External` interface with the scripted
  `Ask`; two slow real-network tests in `check.rs` that they replace were
  deleted; the remaining real-network tests are the `Http` adapter's
  end-to-end tests through `check::document` and `Preview`. Docs: the code map
  and the dependency table.

## Where the difficulty was

- **Not in the cycle.** Breaking it was moving code: `title` had no
  dependencies, and the `<Notes>` scanner was already a self-contained block
  in `widgets.rs`.
- **In the call sites.** Render's signature change reached 18 test calls in
  `render.rs`, plus `search.rs`, `check.rs` and the server; the router's new
  argument reached five test routers in four files. Mechanical, but it is
  where the time went. Every later step that changes a constructor will pay
  the same — four router constructors with six or seven positional arguments
  each (stage 1 step 3 collapses them).
- **Tests that leaned on the globals.** The preview tests shared one
  process-wide channel and answer table; giving each test its own `Preview`
  made them independent, and the scripted `Ask` made the retry rules testable
  in milliseconds instead of with a stub web server and real timeouts.

## What the design got wrong or left open

- **The renderer was not calling the network directly**; it called
  `check::preview`, which spawned background checks only inside a runtime. The
  real coupling was render → check (the cycle), not render → network. The plan
  row's wording ("rendering a page starts no network request") holds now, but
  the fix was moving the call to the server, not changing check.
- **`Preview` still prints each new problem to the terminal** (`eprintln!`).
  It is a side effect in the core; it belongs behind the same seam as logging,
  which no step of the plan names.

## What is left for production quality

- Move the `<Notes>` tests from `widgets.rs` to `markup.rs`, where the
  function now lives (they import it for now).
- `check.rs` still depends on `render` (to render a page and collect its ids)
  and on `comments`, `designs`, `navbar`, `widgets` — fine for direction, but
  the module is still large; step 3 or a later one should split the internal
  link walk from the per-kind checks.
- `Preview::shared()` is a test helper on the production type; a test module
  helper would be cleaner.
- The terminal reporting above.
