# The vendored pele bundle

What pele is in devpage — a third-party JavaScript bundle copied into the
repository that draws Mermaid diagrams in the browser — where it came from,
its version and licences, how devpage calls it, and how to upgrade it. It is a
mechanism page under the [viewer](../README.md), for someone about to replace
the bundle or change how diagrams are drawn; how to write a diagram is in
[Diagrams and themes](../diagrams-and-themes/README.md).

## What is in the repository

| File | What it is |
| --- | --- |
| `assets/vendor/pele.js` | The bundle, minified, one file. |
| `assets/vendor/pele.LICENSE` | Its licence text. |

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

`src/server.rs` compiles the bundle into the binary with `include_str!` and
serves it at `/__devpage/pele.js` — see
[built-in assets](../serving-pages/README.md#built-in-assets). Every page loads
it before `devpage.js`. One function of it is used, by the `Diagram`
[system widget](../widgets/README.md#system-widgets) (`assets/widgets/Diagram.tsx`)
that every `<pre lang="mermaid">` block is mounted as: `pele.render(source)`
returns an object whose `svg` string the widget renders, and a thrown error
becomes the
[diagram error block](../diagrams-and-themes/README.md#when-a-diagram-fails). The bundle exports more — zoom
controls, mounting, registering diagram types — and devpage calls none of it.

pele colors its SVG with `--pele-*` CSS variables; `assets/style.css` sets
them from devpage's theme tokens on every `.diagram`, and fixes
`--pele-radius` at `8px`. The mapping is in
[How diagrams follow the theme](../diagrams-and-themes/README.md#how-diagrams-follow-the-theme).

## Upgrading it

There is no package manager step and no script; an upgrade is done by hand:

<Steps>

1. Replace the files
   Put the new bundle at `assets/vendor/pele.js` and its licence at
   `assets/vendor/pele.LICENSE`. Check the new header line and the licence for
   the version and for any newly bundled third-party notices.

2. Rebuild and restart
   The bundle is embedded with `include_str!`, so a running devpage keeps
   serving the old one until it is rebuilt and restarted (`cargo run -- docs`,
   or `cargo install --path .` again for an installed copy).

3. Re-run the diagram check
   Run the Node command in
   [Diagrams and themes](../diagrams-and-themes/README.md#writing-a-diagram)
   over every page that holds a diagram, and confirm each diagram renders and
   each one's `click` count equals its link count.

   $ grep -rl '```mermaid' docs

4. Re-check the `--pele-*` mapping
   List the variables the new bundle reads and compare them with the ones
   `assets/style.css` sets. A variable the stylesheet doesn't set keeps pele's
   own default, which doesn't follow devpage's light and dark tokens, so look
   at a diagram in both themes.

   $ grep -o -- '--pele-[a-z-]*' assets/vendor/pele.js | sort -u
   $ grep -o -- '--pele-[a-z-]*' assets/style.css | sort -u

5. Update the docs
   The pages that name the version — this page and
   [Diagrams and themes](../diagrams-and-themes/README.md#writing-a-diagram).

</Steps>

The bundle 1.10.0 reads these variables today: `--pele-bg`, `--pele-fg`,
`--pele-muted`, `--pele-line`, `--pele-surface`, `--pele-surface-alt`,
`--pele-border`, `--pele-accent`, `--pele-font`, `--pele-font-mono`,
`--pele-radius`, plus some devpage leaves at pele's defaults — `--pele-error`,
`--pele-critical`, the `--pele-series-*` colors and the `--pele-zoom-*`
settings.
