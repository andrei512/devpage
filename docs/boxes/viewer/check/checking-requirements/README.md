---
page-type: task
---
# Checking requirements

The people who run a system say what it must do; those statements are its
requirements. A site can keep them as pages and have devpage hold the system
to the ones a test can check:

- A **constraint** is a requirement a test can check.
- A test points at the constraint it checks.
- `devpage check` reports a constraint no test points at, and every test that
  fails.

Requirements are optional: a site that configures none is checked as before.
This page is how to set them up. It sits under [Check](../README.md).

## Three kinds of requirement

A requirement is a page, and its [page type](../../glossary/README.md#page-type)
says how anyone finds out whether it holds:

| Kind | Holds when | Found out by |
| --- | --- | --- |
| [Constraint](../../glossary/README.md#constraint) | the system does what it says | a test |
| [Review rule](../../glossary/README.md#review-rule) | a change keeps to its words | a reviewer reading the change |
| [Objective](../../glossary/README.md#objective) | a number is reached | a benchmark, or a data source outside the repository |

When a test can check a requirement, it is a constraint; a review rule is
only for what no test can check. An objective can have
[processes](../../glossary/README.md#process) under it: pages an agent follows to
work toward the number, alone on a schedule or in a session with a person.

devpage checks constraints. It reads a page as one by its front matter:

```markdown
---
page-type: constraint
id: a-published-version-never-changes
---
# A version never changes
```

`id` is the name tests know the constraint by. It is the page's own, so the
file can be renamed or moved without touching a test.

The other kinds are page types of the site's own
([Structuring a site](../structuring-a-site/README.md)): review reads a review
rule, and an objective's page says where its number comes from.

## Turning it on

A site turns requirements on in its document file, under `extensions`, with
one entry per language or test framework:

```yaml
kind: site
title: Handbook
root: README.md
extensions:
  requirements:
    config:
      tests:
        - name: rust
          glob: "../src/**/*.rs"
          pattern: '#\[spec\("([^"]+)"\)\]'
          run: "cargo +nightly test -- -Z unstable-options --format junit"
```

| Field | Meaning |
| --- | --- |
| `name` | What the suite is called in problems. |
| `glob` | Its test files, relative to the document file. |
| `pattern` | A regular expression that finds, in those files, where a test names a constraint. Its first group is the constraint's id. |
| `run` | The command that runs the tests and prints their results as JUnit XML on standard output. It runs in the document file's folder. |

The pattern is the site's own, so a test names its constraint the way the
language makes natural: a comment, an attribute, a decorator.

## A test points at a constraint

A test names a constraint by its `id`. With the pattern above:

```rust
#[test]
#[spec("a-published-version-never-changes")]
fn a_published_version_reads_back_unchanged() {
```

Here `spec` is an attribute that does nothing, from the
`codespec-rust-macro` crate; devpage only reads the text. A comment the
pattern matches works as well.

Several tests may point at one constraint, and the constraint's page lists
none of them: the tests name the constraint, never the other way around.

## What is checked

`devpage check` reports these like a broken link, and exits `1` when there is
any:

| What | Must | Reported as |
| --- | --- | --- |
| A constraint | have an `id` | `<page>: a constraint has an id — its front matter has no id` |
| A constraint | have at least one test pointing at it | `<page>: no test points at this constraint — a test names it <id>` |
| An id a test names | be a constraint's | `<test file>:<line>: <id> — no constraint has this id` |
| A constraint's id | be its alone | `<page>: constraint id <id> is taken by <page>` |
| Every test a suite runs | pass | `<document file>: tests <name>: <test> failed` |
| A suite's `run` | print a JUnit report | ``<document file>: tests <name>: `<run>` printed no JUnit report (exit code <n>)`` |

So a constraint is met when at least one test points at it and no test fails.

## Where a requirement lives

A requirement belongs to a part of the system, and the folder says which: a
page in `<part>/requirements/` is a requirement of the part whose folder that
is — the one its `README.md` describes — and of every part inside it. So a
requirement goes in the smallest part that holds everything that could break
it, and nothing links a requirement to its part by hand.

The part's page shows them with the
[`Requirements`](../../on-the-page/widgets/system-widgets/README.md#requirements) widget:
its own requirements, then those of each part above.

```md
## Requirements

<Requirements />
```

## Seeing what is met

`devpage check` keeps what its run found in
`.devpage/requirements/<document file>.json` at the top of the served folder,
beside the comments and as far from git. The `Requirements` widget reads it:
each constraint shows as met, not met, without a test or not checked, with the
time of the run under the list. The test a pointer names is the first test of
the JUnit report whose name comes after the pointer in its file.

The results are this machine's last run: `devpage check --docs` runs no test
and keeps nothing, and a change made since the run shows only after the next.

These docs do this: [`docs/devpage.yml`](../../../../devpage.yml) reads every
`#[spec("…")]` in the repository's Rust files and runs `cargo test`.

`devpage check` runs every suite, which takes as long as the tests do.
`devpage check --docs` checks the docs alone — links, diagrams, page types —
and leaves requirements and tests out.

Use `devpage check --requirements-map` to validate the requirement IDs and the test files that refer to them. This reads the configured files and reports missing or duplicate IDs. The last recorded suite results stay available.
