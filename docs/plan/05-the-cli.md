---
id: "05"
blocked-by: []
boxes: [viewer/server]
---

# The CLI: contexts and every server command

Built against the API as the
[reference](../boxes/viewer/server/reference/README.md) pins it; requests the
server doesn't answer yet (lanes 01 and 03 add them) are tested against a fake
server answering the pinned contract.

## Build

- `devpage context add|use|list`, `~/.devpage/config.yml`,
  `--token-command` run before each request and sent as a bearer token,
  `--context` and `DEVPAGE_CONTEXT`.
- `devpage publish <path> [--id] [--new]`: runs `devpage check` (external
  addresses too), zips exactly the files the check followed, uploads, prints
  the link, and remembers path → id per context in `published.yml`.
- `list`, `versions`, `comments` (+ `reply`, `resolve`), `download`, `delete`;
  every command `--json`. Ids, never paths, except for `publish`.

## Acceptance

- Tests: the zip holds exactly the checked files (a stray secret beside the
  document is not in it); a broken link stops publish before any request;
  `published.yml` per context; `--id`/`--new`; the token command's output in
  the request; each command's text and `--json` output against the fake
  server, and `publish` against a real `devpage server` on `file://`.
- The `server` subcommand from lane 01 and these commands coexist in
  `main.rs` — lane 01 touches it too.
