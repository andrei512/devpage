---
page-type: reference
---
# Review bars and CI

The two bars a change is reviewed against — one for a ticket going into a
design branch, one for a design branch going into `main` — and what checks
them today. It sits under the [SDLC](../README.md); how a
review runs is the [Review stage](../stages/README.md#review).

## The two bars

| Bar | Applies to | Asks |
| --- | --- | --- |
| **Task bar** | A ticket going into a design branch, and a direct pull request | Acceptance criteria pass, the constraints of the boxes it touches hold, gates green, the system still works. |
| **Drift bar** | A design branch going into `main` | The code does what the docs claim, every expand–migrate–contract finished, no old path left beside its replacement. |

## CI

The workflows run on GitHub, on GitHub-hosted runners only — the repository
is public and never uses a self-hosted runner.

**`.github/workflows/gates.yml`** runs on every pull request and every push to
`main`, `design/**` and `prototype/**` — tickets merge into a design branch
without a pull request, so a push there is what gates them. A manual dispatch
or the weekly schedule requests full coverage.

- **`select`** computes the [affected checks](../../developing-devpage/README.md#the-gates)
  and publishes the decision in the workflow summary. It shares the exact plan
  with the platform jobs. A pull request compares with the merge base of its
  head and target branch. A push compares with a previous successful push of
  this workflow on the same branch. An unavailable baseline widens coverage.
- **`gates`** executes the selected Rust checks on Linux and Windows. Linux
  handles docs, selector tests and live-session Bun checks. Windows reuses
  the compiled test binary for the selected browser checks, running the union
  of their filters in a single Cargo process and skipping GPU tests. An empty
  selection succeeds before the platform jobs need toolchains.
- **`install`** runs `cargo install --path . --locked` and `devpage --version`
  in the bare Rust container for selected packaging changes, keeping the
  [install page](../../../../installing.md) accurate.

Affected checks are the default. To force full coverage temporarily, set the
repository Actions variable `DEVPAGE_CHECKS_MODE` to `full`. The older
`DEVPAGE_BROWSER_MODE` is consulted when the new variable is unset. Its
`shadow` value requests broad coverage. Manual and scheduled full runs work
independently of the variable. Both selector configuration errors and failed
checks fail their workflow.

Cargo jobs reuse `Swatinem/rust-cache`, pinned by commit, for dependencies
matching the lockfile and toolchain. Only main saves the cache. The clean
install job excludes installed binaries so that its install always executes.

**`.github/workflows/deploy-example.yml`** keeps the
[example deployment](../../../viewer/share/deploying/README.md) true. Its one job,
**`image`**, builds the image in `deploy/` and starts it behind its proxy
with Docker Compose, then checks that the docs load, that a design's PNG is
served as `image/png`, that a comment is accepted, and that
`devpage check docs` passes inside the container. The image installs the
released crate from crates.io, so a change outside `deploy/` can't change
what it tests: it runs when a pull request or push to `main` changes `deploy/bundled/`
or its workflow, on a manual dispatch, and every Monday at 06:00 UTC. A release
pull request, which moves the image to a version not yet published, fails it
until the version is.

**`.github/workflows/deploy-server.yml`** keeps the
[server example](../../../viewer/share/server/running-a-server/README.md) true: it
builds the image in `deploy/server/` from the checkout on x86-64, publishes
into both emulated buckets and downloads a version's PDF, when the selector chooses the smoke on a pull request or push to `main`.
The job starts on every PR, so its successful zero-check result is visible
for unrelated changes.

**`.github/workflows/release.yml`** builds a release's
[prebuilt binaries](../stages/README.md#release) when a version tag is pushed.
On a pull request it only plans the release; the gates build Linux and
Windows, and macOS is where devpage is developed.

**`.github/workflows/bench.yml`** is [the server's benchmark](../../tests/README.md#the-servers-benchmark),
run by hand from the Actions tab: it reports a server's numbers against the
envelope's targets and against the last run from `main`. It gates nothing on
its own; a change to how a server serves, publishes, caches or holds memory
links its report from the pull request.

<Callout kind="note" title="The GPU tests run by hand">

CI runs the Chrome tests on Windows, but not those that draw on the GPU, nor
any on Linux or macOS: a GitHub-hosted runner has no GPU, and WARP, Windows'
software adapter, neither draws in time nor draws glyph edges as a GPU does.
Whoever changes what the GPU tests cover runs them by hand on a machine with
one ([Developing devpage](../../developing-devpage/README.md#tests) says when).

</Callout>
