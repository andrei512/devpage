# 01 — Quick fixes

Three independent fixes; one lane each, in parallel.

## 01a. A pinned Chrome

On every machine — even with browsers cached — `pdf`, slide PNGs, `video` and
the Chrome tests fail: chromiumoxide 0.9.1's fetcher resolves "current
Stable" via `https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions.json`
(returns an empty 200 today) *before* checking its cache
(`chromiumoxide_fetcher` `fetcher/mod.rs`, `version/mod.rs`). Offline runs
never worked for the same reason.

Fix: `pub(crate) const CHROME_VERSION: (u32, u32, u32, u32) = (156, 0, 8078, 47);`
in `src/pdf.rs`, used by both `headless_shell()` and `chrome_for_testing()`
via `.with_version(BrowserVersion::Version(Version::exact(..)))` — an exact
version resolves with no network call, the cache is checked, and only a
missing browser downloads the per-version zip
(`storage.googleapis.com/chrome-for-testing-public/<v>/<platform>/…zip`,
verified 200 for mac-arm64, mac-x64, linux64, linux-arm64). Existing cache
folders (`mac_arm-156.0.8078.47`) are reused. Errors name kind, version,
platform and URL; an unsupported platform (the fetcher has no Linux arm64)
says so. No Cargo changes.

Tests: unit (pinned version → expected folder/executable under a temp cache,
no network); `#[ignore]` network HEAD of both zips for every supported
platform at `CHROME_VERSION`. Docs: installing, pdf, making-a-video ("pinned",
not "latest Stable"), dependencies (who bumps: with chromiumoxide upgrades),
tests.

## 01b. A working example image and install page

`deploy/Dockerfile` doesn't build: `parley`'s default `fontique/system` pulls
`yeslogic-fontconfig-sys` (needs pkg-config + libfontconfig). devpage never
uses system fonts (`design/src/render.rs` sets `system_fonts: false`, Inter is
built in).

Fix:
- `design/Cargo.toml`: `parley = { version = "0.12.0", default-features = false, features = ["std"] }`
  (verified: 66 design tests pass, reference PNGs unchanged, nothing links
  fontconfig). Check `cargo run -p devpage-design-module` leaves the vendored
  wasm unchanged.
- `deploy/Dockerfile`: install from crates.io at `ARG DEVPAGE_VERSION`, take
  `docs/` and `LICENSE` from that crate's own source
  (`$CARGO_HOME/registry/src/*/devpage-$DEVPAGE_VERSION`) so binary and docs
  match; runtime adds `ca-certificates` (the link checker panics without it);
  fontconfig packages stay while the published version still needs them.
  Ready-made in `/private/tmp/claude-501/-Users-andrei-projects-morph/51641adb-85d1-43f2-bcf1-b8b7caa2762a/scratchpad/team-f/`.
- Docs: deploying (new Dockerfile; docs come from the published crate;
  `--no-comments` needs the release after 0.2.0; slide PNGs and pdf aren't in
  this image); installing (Linux needs pkg-config + libfontconfig1-dev for
  releases up to 0.2.0).
- CI: `.github/workflows/deploy-example.yml`, **`runs-on: ubuntu-latest`
  only — the public repo never uses a self-hosted runner**; on PR, push to
  main and weekly: build the image, `compose up`, expect 200 for `/`, a design
  PNG as image/png, a comment POST; `devpage check docs` in the image; and a
  `cargo install --path . --locked` job in `rust:1.97-slim-bookworm` with only
  the prerequisites installing.md lists.

## 01c. One tab title

A site page's tab is `<page> · <site>` (`src/render.rs:231`), so the docs root
reads "devpage · devpage". Rule: show just the site name when the page title
equals it (trimmed, case-insensitive). A `tab_title(page, site)` helper;
tests for equal / different / case-only; configuration-reference line 86.
