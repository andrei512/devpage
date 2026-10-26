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

One workflow runs on GitHub, `.github/workflows/deploy-example.yml`, on every
pull request, every push to `main` and every Monday at 06:00 UTC, on
GitHub-hosted runners only — the repository is public and never uses a
self-hosted runner. It keeps the [example deployment](../../../viewer/deploying/README.md)
and the [install page](../../../../installing.md) true, and nothing else:

- **`image`** builds the image in `deploy/` and starts it behind its proxy
  with Docker Compose, then checks that the docs load, that a design's PNG
  is served as `image/png`, that a comment is accepted, and that
  `devpage check docs` passes inside the container.
- **`install`** runs `cargo install --path . --locked` in a bare
  `rust:1.97-slim-bookworm` container, with nothing added, and runs
  `devpage --version`.

<Callout kind="note" title="The gates run by hand">

The workflow doesn't run the gates. The gates in `AGENTS.md`
(`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo test`, and `cargo test -- --ignored` where Chrome is concerned) are run
by hand, by whoever is about to merge or open a pull request;
[Developing devpage](../../developing-devpage/README.md#the-gates) shows them.

</Callout>
