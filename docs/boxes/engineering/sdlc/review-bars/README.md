# Review bars and CI

The two bars a change is reviewed against — one for a ticket going into a
design branch, one for a design branch going into `main` — and what checks
them today. It is a reference page under the [SDLC](../README.md); how a
review runs is the [Review stage](../stages/README.md#review).

## The two bars

| Bar | Applies to | Asks |
| --- | --- | --- |
| **Task bar** | A ticket going into a design branch, and a direct pull request | Acceptance criteria pass, the constraints of the boxes it touches hold, gates green, the system still works. |
| **Drift bar** | A design branch going into `main` | The code does what the docs claim, every expand–migrate–contract finished, no old path left beside its replacement. |

## CI

Four workflows run on GitHub, on GitHub-hosted runners only — the repository
is public and never uses a self-hosted runner.

**`.github/workflows/gates.yml`** runs on every pull request and every push to
`main`, `design/**` and `prototype/**` — tickets merge into a design branch
without a pull request, so a push there is what gates them. Its jobs run
side by side:

- **`gates`** runs the gates in `AGENTS.md` — `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings` and `cargo test`, with
  `--locked` — and then `devpage check docs`, external links included, on
  the change's own docs. It runs twice, on Linux and on Windows: devpage
  runs on both and on macOS, where it is developed.
- **`install`** runs `cargo install --path . --locked` in a bare
  `rust:1.97-slim-bookworm` container, with nothing added, and runs
  `devpage --version`, keeping the [install page](../../../../installing.md)
  true.
- **`install-windows`** installs devpage from the checkout on Windows the
  same way.
- **`chrome-windows`** runs the Chrome tests on Windows but the GPU's,
  `cargo test --bin devpage -- --ignored --skip gpu`: every test that draws
  on the GPU has `gpu` in its name, and the runner has no GPU.

Every job keeps the built dependencies between runs with `Swatinem/rust-cache`,
pinned to a commit, keyed on `Cargo.lock` and the toolchain, and saved when a
run fails too, so a red run doesn't leave the next one to build them all
again; devpage's own
crates are built afresh every run. A change to `Cargo.lock` or the toolchain
misses the cache, so `install` builds every dependency again — their build
scripts and C sources too — with nothing added. `install` caches no installed
binary: one would make the install a no-op.

**`.github/workflows/deploy-example.yml`** keeps the
[example deployment](../../../viewer/deploying/README.md) true. Its one job,
**`image`**, builds the image in `deploy/` and starts it behind its proxy
with Docker Compose, then checks that the docs load, that a design's PNG is
served as `image/png`, that a comment is accepted, and that
`devpage check docs` passes inside the container. The image installs the
released crate from crates.io, so a change outside `deploy/` can't change
what it tests: it runs on a pull request that touches `deploy/` or the
workflow, on every push to `main`, and every Monday at 06:00 UTC. A release
pull request, which moves the image to a version not yet published, fails it
until the version is.

**`.github/workflows/deploy-server.yml`** keeps the
[server example](../../../viewer/server/running-a-server/README.md) true: it
builds the image in `deploy/server/` from the checkout on x86-64, publishes
into both emulated buckets and downloads a version's PDF, on a pull request
that touches the code or `deploy/server/`, and on every push to `main`.

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
