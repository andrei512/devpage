# Upstream provenance and devpage integration

Source: [mattpocock/skills](https://github.com/mattpocock/skills/tree/5beb35b040e3de97e54fa9e386d7e42d15c295c2/skills/engineering/improve-codebase-architecture)

Pinned commit: `5beb35b040e3de97e54fa9e386d7e42d15c295c2`.

`SKILL.md`, `HTML-REPORT.md`, and `agents/openai.yaml` are unchanged upstream
files. `LICENSE` is the upstream repository's MIT license at that commit.
This note is devpage-specific.

## Use in devpage

Read this integration note **before** the upstream skill. Architecture reviews
use the upstream skill alongside the [review skill](../review/SKILL.md).
devpage's review scope, evidence bar, architecture constraints, and permission to
write govern both bundled skills.

- For the upstream `codebase-design` call, read the bundled
  [codebase-design skill](../codebase-design/SKILL.md), including its linked
  deepening guidance; its alternative-interface pattern applies only when that
  exploration is in scope.
- For `GLOSSARY.md` and `docs/adr/`, use the relevant box glossaries and design
  docs under `docs/boxes/`. devpage records decisions in box docs and has no ADR
  directory. Do not create those upstream paths.
- For the upstream `grilling` call after a candidate is selected, use devpage's
  [align skill](../align/SKILL.md) to discuss the decision with the operator.
- For `domain-modeling` and proposed inline glossary/decision writes, use devpage's
  [design skill](../design/SKILL.md): discuss first, then update the relevant box
  docs only when the operator asks for the write. Do not write as each point lands.

`grilling` and `domain-modeling` are deliberately replaced by these existing
devpage workflows. The upstream files remain unchanged; these integration rules
resolve their project-specific assumptions. Installing these skills does not
authorize an architecture scan, design update, or implementation.
