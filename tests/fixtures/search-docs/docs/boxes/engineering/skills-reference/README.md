# Skills reference

Every skill in this repository — who starts it, what it takes and what it
produces — where the skill files live, and the two skills taken unchanged from
an upstream repository. It is the reference page under
[Engineering](../README.md); [Running the flow with skills](../running-the-flow/README.md)
explains how they are used together.

## Where skills live

<FileTree>

- .claude/
  - skills/
    - design/
      - SKILL.md — frontmatter (`name`, `description`, sometimes `disable-model-invocation: true`), then the instructions
    - … one folder per skill, some with supporting files
- .agents/
  - skills — a symbolic link to `../.claude/skills`, for tools that look under `.agents/`
- AGENTS.md — the working rules for every agent: talking to the operator, the flow, the gates, language, the docs
- CLAUDE.md — a symbolic link to `AGENTS.md`

</FileTree>

There is one copy of each skill; edit it under `.claude/skills/`.

## The skills

The operator starts a stage; agents reach for the skills that serve one. That
split is machine-readable: an *operator only* skill carries
`disable-model-invocation: true` in its frontmatter, so an agent never starts
it on its own.

### Operator only

The operator's slash command starts these.

| Skill | Takes | Produces |
| --- | --- | --- |
| `design` | An idea for how the system should change. | A conversation until the understanding is shared, then — when asked — box docs and constraints written on `design/<name>`. |
| `plan` | `docs/plan/00-prototype.md` on the design branch. | A story the operator approves, and one ticket file per step in `docs/plan/`, with `id`, `blocked-by` and `boxes` front-matter. |
| `build` | An approved plan. | Lanes run by forks in their own worktrees, merged `--no-ff` one at a time; `docs/plan/` deleted; a design-branch pull request to `main` reviewed at the drift bar; after the operator's merge, the branches deleted and the retro. |
| `hack` | A bug or small change. | Aligned, then built with the operator in their checkout on `hack/<name>`; docs, tests and gates paid before a pull request to `main`. |
| `improve-codebase-architecture` | The codebase, or a direction the operator names. | Deepening opportunities as an HTML report in the system temp folder, then a discussion of the one the operator picks. Upstream skill. |

### Operator or agent

An agent may also reach for these when its work calls for one.

| Skill | Who reaches for it | Takes | Produces |
| --- | --- | --- | --- |
| `prototype` | The operator asks, and the design session forks itself to run it | A settled design on its design branch. | One end-to-end path in real code with tests, built in thirty measured minutes on `prototype/<name>`; the report `docs/plan/00-prototype.md`; the branch merged into the design branch and deleted. |
| `implement-ticket` | A lane's fork | One ticket, in its own worktree, on a branch off the design branch. | The ticket built red-green, gates green, self-reviewed at the task bar; a branch ready to merge and a report of what changed and what was left alone. |
| `review` | The operator, or any agent | A branch or pull request and its target. | Findings, each demonstrated, against the task bar (into a design branch) or the drift bar (into `main`). |
| `retro` | The session that coordinated `build` | A design branch that has merged into `main`. | A short report in chat: one line per stage, its cost and what it caught. Writes nothing. |
| `align` | Used inside every stage | A conversation. | Shared understanding, one topic at a time. Writes no files and authorizes nothing. |
| `documentation` | The operator, or any agent | A documentation subject and its scope. | Pages written to the five-layer reading hierarchy, from landing to reference. |
| `presentation` | The operator, or any agent | A deck to make or improve, its purpose and its audience. | The story agreed with the operator (shaped with marketing and developer-relations viewpoints), slides of one headline and one picture each, speaker notes in plain English, every slide checked as an image. |
| `codebase-design` | The operator, or any agent | Code being designed or restructured. | A vocabulary — module, interface, depth, seam, adapter, leverage, locality — and principles for deep modules. Upstream skill. |

`AGENTS.md` names when the upstream architecture skill is used: architecture
reviews, alongside `review`, after reading its integration note.

## The upstream skills

`codebase-design` and `improve-codebase-architecture` are copied from the
[mattpocock/skills](https://github.com/mattpocock/skills) repository. Each
folder's `UPSTREAM.md` records:

- **Source and pinned commit** — the skill's folder in that repository at
  commit `5beb35b040e3de97e54fa9e386d7e42d15c295c2`.
- **What is unchanged** — `SKILL.md`, `DEEPENING.md`, `DESIGN-IT-TWICE.md`
  and `agents/openai.yaml` in `codebase-design`; `SKILL.md`, `HTML-REPORT.md`
  and `agents/openai.yaml` in `improve-codebase-architecture`.
- **Licence** — the upstream repository's MIT licence at that commit, copied
  as each folder's `LICENSE`.

The upstream files assume a project that keeps a `GLOSSARY.md`, ADRs under
`docs/adr/` and its own `grilling` and `domain-modeling` skills. devpage has
none of those, so `improve-codebase-architecture/UPSTREAM.md` adds a
**devpage integration note**, to be read before the upstream skill, that maps
each assumption onto this repository: box glossaries and box docs instead of
`GLOSSARY.md` and ADRs (and never create those paths), `align` instead of
`grilling`, `design` instead of `domain-modeling`. devpage's review scope,
evidence bar and permission to write govern both skills, and installing them
authorizes no scan, design update or implementation. No procedure for moving
them to a newer upstream commit is written down yet.
