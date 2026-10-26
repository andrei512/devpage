# 05 — Fast slide PNGs

Builds on 01a (a browser that downloads).

## Today

`<deck>.yml/<n>.png` takes ~21 s on the devpage talk: `READY`
(`src/pdf.rs:83`) waits for every `canvas.design` to be painted; a design on a
slide that isn't shown has no layout, `fit()` (`assets/design.js:270`) returns
false and it never paints; the poll (`src/slide_png.rs:65-71`, 400 × 50 ms)
times out and screenshots anyway. Shooting the design's own slide: ready in
0.24 s. A design with problems never paints either, costing a PDF the same
20 s. Each request launches a fresh Chrome. A slide whose `<img>` is its own
deck's PNG recursed without end.

## Design

1. One readiness rule for both routes: only laid-out canvases count —
   `ready && [...canvas.design].filter(c => c.checkVisibility()).every(c => c.dataset.painted)`.
   `design.js` settles a design that shows problems (`data-painted="problems"`)
   and clears `painted` when a refit is pending. On timeout, log what was
   missing (and an `X-Devpage-Ready: timeout` header on the PNG).
2. One shared browser in the server state: launched lazily, one tab per shot
   (closed after), a semaphore of 2, relaunched if its handler ends, closed
   after ~60 s idle and on shutdown; its profile lives as long as it does.
3. A cache keyed by (deck, n), cleared on any change event, size-bounded; an
   ETag so a repeat request is a 304 without rendering.
4. Recursion guard: the shot tab sends `X-Devpage-Shot: 1`
   (`Network.setExtraHTTPHeaders`); the PNG route answers such a request from
   cache or with 508 and never starts a nested shot. `devpage pdf` sets it too.

## Tests (Chrome `#[ignore]`)

A deck with a design on slide 2: slide 1's PNG in under 3 s, slide 2's shows
the design painted; a design with problems doesn't delay a PDF; a slide
embedding its own PNG returns promptly; a repeat request comes from cache, and
a file change clears it.

## Docs

making-slides ("A slide as an image": readiness, caching, the recursion
rule), pdf (readiness wording), serving-pages (the 508 for a nested shot).
