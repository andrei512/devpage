---
id: "02"
blocked-by: []
boxes: [viewer/design-engine, viewer/live-updates, viewer/checking-links, viewer/configuration-reference]
---

# The server keeps designs resolved and resolves only what a save touched

- The watcher hands each changed path to the design store; only designs made
  from that file are parsed and resolved again, and the store caches parsed
  files by path. A save that changes nothing in a scene sends nothing.
- `Watch`: whole scene first, then patches of changed subtrees, problems as
  their own update, the stream ending with `no such file` when the design is
  deleted.
- `GET <path>.design.png`: the PNG at 1×, or `422` with the problems as plain
  text, one per line.
- `devpage path/x.design` serves that design as the entry document (`/` leads
  to it), and `devpage check` reports the problems of every design a page links
  to or embeds, and of the designs those import.

## Acceptance

- Route tests over HTTP: PNG 200 and 422; a Watch call reading scene, then a
  patch after a save of an imported design, then nothing after a no-op save,
  then problems, then the deleted-design end.
- A check test: a page embedding a broken design fails `devpage check` with
  the design's `file:line:col` message.
- The pre-existing watcher test failures on this machine (they fail on main
  too) are diagnosed: fixed if they are a real bug, otherwise the cause is
  stated in the lane report.
