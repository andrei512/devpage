<Slide title="The code is the reference.">

<div style="display:grid;grid-template-columns:1fr 1fr;gap:70px;width:100%;align-items:center;font-size:34px">
<div>

```python
class Graph:
    """A calculation that returns
    its current value."""

    def run(self, value):
        """Return the supplied value."""
        return value
```

</div>
<div style="padding:48px;border:1px solid var(--border);border-radius:20px;background:var(--surface)">

Class

## Graph

A calculation that returns its current value.

### Methods

[**run(self, value)**](../../reference/python/sample/graph/Graph/run)

Return the value supplied for this run.

</div>
</div>

</Slide>

<Notes delivery="Concrete and calm: open the linked method if presenting live.">
The docstring beside the code becomes the reference page. This link opens the
real Python example's method, through the same pages and search as Rust and
TypeScript. Project extensions can supply domain declarations, while devpage
still checks their links and keeps their explanatory prose with them.
</Notes>
