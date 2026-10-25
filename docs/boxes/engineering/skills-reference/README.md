# Skills reference

Every skill in this repository — who starts it, what it takes and what it
produces — where the skill files live, and the two skills taken unchanged from
an upstream repository. It is the reference page under
[Engineering](../README.md); [Running the flow with skills](../running-the-flow/README.md)
explains how they are used together.

## Where skills live

| Path | What it is |
| --- | --- |
| `.claude/skills/<name>/SKILL.md` | One folder per skill. The frontmatter holds `name`, `description` and, for some, `disable-model-invocation: true`; the body is the instructions. Some folders hold supporting files beside it. |
| `.agents/skills` | A symbolic link to `../.claude/skills`, so tools that look for skills under `.agents/` read the same files. |
| `AGENTS.md` | The repository's working rules for every agent: talking to the operator, the flow, the gates, language, where the docs are. |
| `CLAUDE.md` | A symbolic link to `AGENTS.md`. |

There is one copy of each skill; edit it under `.claude/skills/`.

## The skills

**Started by** says who may start a skill. *Operator only* means the skill
carries `disable-model-invocation: true`: an agent doesn't start it on its own,
the operator's slash command does. *Operator or agent* means an agent may also
reach for it when its work calls for it.

| Skill | Started by | Takes | Produces |
| --- | --- | --- | --- |
| `design` | Operator only | An idea for how the system should change. | A conversation until the understanding is shared, then — when asked — box docs and constraints written on `design/<name>`. |
| `prototype` | Operator or agent — the operator asks, and the design session forks itself to run it | A settled design on its design branch. | One end-to-end path in real code with tests, built in thirty measured minutes on `prototype/<name>`; the report `docs/plan/00-prototype.md`; the branch merged into the design branch and deleted. |
| `plan` | Operator only | `docs/plan/00-prototype.md` on the design branch. | A story the operator approves, and one ticket file per step in `docs/plan/`, with `id`, `blocked-by` and `boxes` front-matter. |
| `build` | Operator only | An approved plan. | Lanes run by forks in their own worktrees, merged `--no-ff` one at a time; `docs/plan/` deleted; a design-branch pull request to `main` reviewed at the drift bar; after the operator's merge, the branches deleted and the retro. |
| `implement-ticket` | Operator or agent — a lane's fork | One ticket, in its own worktree, on a branch off the design branch. | The ticket built red-green, gates green, self-reviewed at the task bar; a branch ready to merge and a report of what changed and what was left alone. |
| `review` | Operator or agent | A branch or pull request and its target. | Findings, each demonstrated, against the task bar (into a design branch) or the drift bar (into `main`). |
| `retro` | Operator or agent — the session that coordinated `build` | A design branch that has merged into `main`. | A short report in chat: one line per stage, its cost and what it caught. Writes nothing. |
| `hack` | Operator only | A bug or small change. | Aligned, then built with the operator in their checkout on `hack/<name>`; docs, tests and gates paid before a pull request to `main`. |
| `align` | Operator or agent — used inside every stage | A conversation. | Shared understanding, one topic at a time. Writes no files and authorizes nothing. |
| `documentation` | Operator or agent | A documentation subject and its scope. | Pages written to the five-layer reading hierarchy, from landing to reference. |
| `codebase-design` | Operator or agent | Code being designed or restructured. | A vocabulary — module, interface, depth, seam, adapter, leverage, locality — and principles for deep modules. Upstream skill. |
| `improve-codebase-architecture` | Operator only | The codebase, or a direction the operator names. | Deepening opportunities as an HTML report in the system temp folder, then a discussion of the one the operator picks. Upstream skill. |

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
