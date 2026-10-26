# 04 — One swap path

Read [the plan](README.md)'s shared spine first. Builds on 02 (ETag skip) and
03 (preload helper).

## Today (assets/devpage.js)

- `go()` (~:715) sets `location.hash`; Chrome fires `popstate` before
  `hashchange`; the `popstate` handler (~:661) calls `load(location.href)`:
  every slide change refetches and rebuilds the page. `load()` (~:216)
  `replaceChildren` puts in `.slide` elements without `shown`
  (`style.css:222` shows only `.slide.shown`) and `showSlide()` runs only after
  `await mountWidgets()` — a frame paints with no slide (measured: one blank
  frame ~140 ms after a key on devpage.ai; locally each key fetched the page
  plus ~30 comment requests).
- Same tear-down on live updates (~:675), a tab shown again (~:690), link
  navigation (~:262), same-page `#anchor` and outline clicks.
- Old React roots are never unmounted (effects, timers and a design Watch
  stream leak per swap).
- Widget placeholders carry `data-source` (`widgets.rs:274`), so `sources()`
  fetches comments for "pages" like `<AgentFiles />` (~20 requests on the
  deck), one after another (`devpage.js:285,380`).

## Design: fetch, stage, commit

1. **History**: keep `shownPath` (pathname + search); `popstate` with the same
   path only runs `showSlide()` and lets the browser scroll to the fragment —
   no fetch. `history.scrollRestoration = "manual"`; save `scrollY` in
   `history.state` before each push, restore on back/forward.
2. **Stage**: parse the fetched page; mount new widgets inside a hidden
   staging element in the same document (`visibility:hidden`, absolute, the
   width of `#devpage-page`, so self-measuring widgets work). **Commit** in one
   synchronous task: `replaceChildren` on page, nav, navbar, `showSlide()`,
   `markBroken`, scroll (kept `scrollY` for a live update; fragment or top for
   navigation), then `drawDesigns()`.
3. **Keep what hasn't changed**: generalise `keepDesigns` to `keepUnchanged`:
   key each top-level placeholder on its server HTML (saved before mount) plus
   its module URL; a match moves the old, still-mounted element in (state
   kept); others mount fresh; roots no longer used are `unmount()`ed (closing
   their design streams).
4. **Concurrency**: `load()` gets a generation counter and an
   `AbortController`; live events during a load coalesce into one follow-up.
   The visibility catch-up uses the same path. 02's ETag skip happens before
   staging.
5. **Comments**: the widget attribute becomes `data-widget-source` (server and
   client), so only real pages are asked for comments; fetch them in parallel.

## Tests (Chrome `#[ignore]`, in the style of comments.rs)

Deck: a key press makes no document fetch and every sampled frame has a
`.slide.shown`. Doc: an outline `#` click makes no fetch. Live update: an
unchanged stateful widget keeps its node and state, a changed one updates,
`scrollY` holds, no sampled frame shows an unmounted placeholder. An edited
widget (new URL) remounts even with unchanged placeholder HTML. Navigation: no
half-built frame; back restores scroll. A design outside a widget keeps its
canvas; a removed widget's design stream closes. Unit: placeholders no longer
emit `data-source`; comments fetched only for page sources.

## Docs

page-script (swapping in place, mounting widgets, links and history),
designs (the "embed inside a widget" note), features/live.md and
features/navigation.md (widget state and scroll survive), the widgets pages
that document placeholder attributes.
