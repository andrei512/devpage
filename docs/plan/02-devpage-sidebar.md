# 02 — `DevpageSidebar`

Read [the plan](README.md) first; it is the design.

## What

1. `DevpageSidebar`, a system widget (`assets/widgets/`, served like the
   others — see `addresses.rs`, `server.rs` `system_widget`), rendering the
   tree as the plan says: collapsed, current branch open, chevrons, open state
   remembered per document, group headings in place.
2. Server side (`src/render.rs` `sidebar`/`tree`): the same markup React will
   produce, collapsed with the current branch open, plus the tree as JSON in
   the page. React takes over from it without the DOM changing.
3. Page swap (`assets/devpage.js`): the sidebar stays mounted across
   navigation; the current-page highlight moves and its branch opens; no
   refetch, no remount.
4. `DevpageSidebar` is reserved: a project widget with that name is a clash,
   reported as other system-name clashes are. A project `Sidebar` is fine.
5. Leave a clear place for 03's search box at the top of the sidebar; don't
   build search.

## Docs (same commit as the code)

- `docs/boxes/viewer/navigation-tree/README.md` and
  `organizing-navigation/README.md`: collapsing, what's remembered, swaps.
- Widgets docs: `DevpageSidebar` is reserved, and the `Devpage` prefix rule
  (devpage's own chrome).
- Page-script box: the sidebar across swaps. Glossary if needed.

## Tests

Red first: server HTML collapsed with the current branch open (unit); a
project widget named `DevpageSidebar` clashes, `Sidebar` doesn't; Chrome tests
(`#[ignore]`, like the others): first paint has the sidebar, React takeover
doesn't change its layout (bounding boxes before and after), a chevron opens
and closes, open state survives a page swap and a reload, the current branch
opens on navigating.

## Done when

Gates green (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo test`, `cargo test -- --ignored` for tests you touched), `devpage check
docs` clean, committed on your lane branch.
