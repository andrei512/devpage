# Notes and slide images

A slide carries its script in a `<Notes>` tag — the words said while it is up,
never shown on it — with a `delivery`, how the slide should feel; a deck says
who its `audience` is, its `goal` and its `tone`. Every slide is also a PNG at `<deck file>/<n>.png`, so an agent
writing the notes can look at the slide it is writing for.

```markdown
# Why devpage

<Notes>
Every team writes docs, and every team's docs drift from the code.
</Notes>
```

<Terminal>

```
$ curl -o slide-3.png localhost:3030/talks/launch/devpage.yml/3.png
```

</Terminal>

Teaches it: [Making slides](../boxes/viewer/making-slides/README.md#speaker-notes).
