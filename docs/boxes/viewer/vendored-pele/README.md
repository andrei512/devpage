# The vendored pele bundle

What pele is in devpage — a third-party JavaScript bundle copied into the
repository that draws Mermaid diagrams on the server — where it came from,
its version and licences, how devpage calls it, and how to upgrade it. It is a
mechanism page under the [viewer](../README.md), for someone about to replace
the bundle or change how diagrams are drawn; how to write a diagram is in
[Diagrams](../diagrams/README.md).

## What is in the repository

| File | What it is |
| --- | --- |
| `core/assets/vendor/pele.js` | The bundle, minified, one file. |
| `core/assets/vendor/pele.LICENSE` | Its licence text. |

The bundle's first line is its only provenance record:

```text
/*! Pele 1.10.0 | MIT License | https://github.com/obsidianmd/pele */
```

So the version is **1.10.0**, the source is that GitHub repository, and the
licence is MIT. `pele.LICENSE` holds the MIT text (copyright kepano) and a
second notice the bundle's header doesn't mention: the reset icon of pele's
zoom controls is Lucide's `rotate-ccw` icon (Lucide v1.14.0), under the ISC
licence. No other file records which commit or release archive the bundle was
taken from.

## How devpage uses it

The bundle is compiled into the binary (`include_str!` in `core/src/diagrams.rs`)
and run by the server only: the [diagram engine](../diagrams/README.md#the-diagram-engine)
loads it into QuickJS once, on its own thread, and draws every `mermaid` fence
with it while it renders a page. It is not a [built-in asset](../serving-pages/README.md#built-in-assets):
no page loads it, and the browser never runs it.

QuickJS is a bare JavaScript engine, with no `URL`. pele uses `URL` for one
thing — to lowercase the scheme and host of an `http:` or `https:` `click`
target — and leaves a target out of the drawing when `new URL` throws, so the
engine defines a small `URL` before loading the bundle: it parses
`scheme://host/rest`, lowercases the scheme and host, and writes the address
back with a space as `%20` and `/` as the path of a bare host, as a browser's
would. Nothing else of the browser is there; pele finds no canvas to measure
text with and sizes labels from its own table of character widths.

One function of the bundle is used: `pele.render(source)` returns an object
whose `svg` string goes into the page, and a thrown error becomes the
[diagram error block](../diagrams/README.md#when-a-diagram-fails) and a
[problem](../diagrams/README.md#checking-diagrams-before-committing). The
bundle exports more — zoom controls, mounting, registering diagram types —
and devpage calls none of it.

pele colors its SVG with `--pele-*` CSS variables, which stay variables in the
SVG; `core/assets/style.css` sets them from devpage's theme tokens on every
`.diagram`. The mapping is in
[How diagrams follow the theme](../themes/how-themes-reach-code-and-diagrams/README.md#diagrams).

## Upgrading it

There is no package manager step and no script; an upgrade is done by hand:

<Steps>

1. Replace the files
   Put the new bundle at `core/assets/vendor/pele.js` and its licence at
   `core/assets/vendor/pele.LICENSE`. Check the new header line and the licence for
   the version and for any newly bundled third-party notices.

2. Rebuild and restart
   The bundle is embedded in the binary, so a running devpage keeps
   drawing with the old one until it is rebuilt and restarted (`cargo run -- docs`,
   or `cargo install --path .` again for an installed copy).

3. Check every diagram
   `devpage check` draws every diagram the docs reach with the new bundle and
   reports each one that no longer parses; the tests in `core/src/diagrams.rs`
   check that a `click` line still becomes a link, and that the engine's
   `URL` is enough for an `https://` target.

   $ cargo run -- check docs
   $ cargo test diagrams

4. Re-check the `--pele-*` mapping
   List the variables the new bundle reads and compare them with the ones
   `core/assets/style.css` sets. A variable the stylesheet doesn't set keeps pele's
   own default, which doesn't follow devpage's light and dark tokens, so look
   at a diagram in both themes.

   $ grep -o -- '--pele-[a-z-]*' core/assets/vendor/pele.js | sort -u
   $ grep -o -- '--pele-[a-z-]*' core/assets/style.css | sort -u

5. Update the docs
   The pages that name the version — this page and
   [Diagrams](../diagrams/README.md#writing-a-diagram).

</Steps>

Which variables the 1.10.0 bundle reads, and which devpage leaves at pele's
defaults, is listed once, in
[How diagrams follow the theme](../themes/how-themes-reach-code-and-diagrams/README.md#diagrams).
