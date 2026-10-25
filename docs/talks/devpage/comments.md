<Columns rows kicker="Comments, with agents" title="Review in the browser. Your agent does the rest.">

```mermaid
flowchart LR
  you["You comment"] --> file["A thread file"]
  file --> agent["The agent reads it"]
  agent --> docs["…edits the docs"]
  docs --> done["…deletes the thread"]
  click you href "../../boxes/viewer/comments/README.md#commenting" "Commenting"
  click file href "../../boxes/viewer/comments/README.md#the-comment-store" "The comment store"
```

```yaml
# .devpage/comments/docs/design.md/20261025T181502Z-7f3a.yml
anchor: { quote: the client retries every write }
thread:
  - { author: Andrei Puni, text: "Every write, or only the outbox?" }
```

</Columns>
