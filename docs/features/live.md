# Live updates

Save a file — by hand or by an agent — and every open page swaps in the new
version, in place, without a reload. The page keeps your place — the scroll,
the slide — and every widget the change didn't touch keeps its state.

```mermaid
flowchart LR
  save["Save"] --> devpage["devpage"]
  devpage -->|"change"| pages["Every open page"]
  click devpage href "../boxes/viewer/live-updates/README.md" "Live updates"
```

Teaches it: [Live updates](../boxes/viewer/live-updates/README.md).
