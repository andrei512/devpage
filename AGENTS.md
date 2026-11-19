# devpage — how we work

devpage is a **live renderer for the documents of software development**:
extended Markdown in the repository — system docs, design docs, PRDs, slides —
rendered in realtime. Vision: `docs/vision.md`.

## Talking to the operator

Short and high-level. A wrong answer is wrong at every zoom level, so the
compressed version is enough to catch it — and cheap to correct. Detail is
**pull, not push**: give the headline and the next decision, stop, and expand
only when asked.

Compression must keep the load-bearing part. "Make diagrams render" is
uncheckable; "a Mermaid fence becomes inline SVG themed by the page's CSS
variables" is checkable.

Never: time estimates, restating the request, listing every file touched,
walls of text, or offering a shortcut instead of the right design.

## The flow

**Design → Prototype → Plan → Build → Review → Release.** One thing at a time,
each stage run by its skill. The process — stages, branch topology, review
bars, and how boxes are documented — is described in `docs/boxes/engineering/sdlc/`.

Two things that hold in every stage:

- **Permission to write is stated, never inferred.** A conversation stays a
  conversation until the operator asks for the artifact.
- **Never report an edit you haven't verified landed.**

## The gates

Use the Node version in `.node-version` and run `npm ci` at the repository root.
During an edit, `npm run check -- --explain` shows which checks the changed
files require. Run them with `npm run check`. Use `npm run build` for the
affected Rust build targets.

Before a pull request, include committed and working-tree changes with
`npm run check -- --base origin/<target-branch>`. Use `-- --full` for full
coverage. A change to test ownership updates the selection rules and their
tests. See [development checks](docs/boxes/engineering/developing-devpage/README.md#the-gates).

Run the docs while you work on them, from the repository root:
`cargo run -- docs`.

## Architecture

Architecture reviews use the bundled
[improve-codebase-architecture skill](.claude/skills/improve-codebase-architecture/SKILL.md)
after first reading its [devpage integration note](.claude/skills/improve-codebase-architecture/UPSTREAM.md)
alongside the review skill.

## Language

**One name per concept, everywhere** — in code, docs and the CLI. A subject's
glossary lives with its box under `docs/boxes/`, and everything else uses those
words.

## Docs

- `docs/vision.md` — ideas, not commitments. The map of where this is going.
- `docs/boxes/` — the design of the system, one directory per box, nested to
  match containment. Format and rules: `docs/boxes/engineering/sdlc/`.
- `docs/requirements/` — the rules that belong to no single box. Read on every
  review, whatever the diff touches.
- There is no decision log and no ADR directory — see
  [decisions-live-in-box-docs](docs/boxes/engineering/sdlc/requirements/decisions-live-in-box-docs.md).
