# Markdown, diagrams and math

GitHub-flavoured Markdown with raw HTML and highlighted code, and `mermaid`
fences drawn as diagrams whose boxes are links.

<Example>

| Kind | Like |
| --- | --- |
| site | a docs website |
| doc | a Word document |

- [x] tables, task lists, footnotes

</Example>

```mermaid
flowchart LR
  md["Markdown"] --> page["A page"]
  click page href "../boxes/viewer/diagrams/README.md" "Diagrams"
```

Math is TeX in the same document: inline \(x_1^2\) or a displayed equation.

\[
f(x)=\frac{1}{\sqrt{2\pi}}e^{-x^2/2}
\]

Native MathML needs no external fonts or browser renderer. Invalid formulas
show their errors and fail `devpage check`.

Teaches it: [Markdown that works](../boxes/viewer/writing-a-document/markdown-that-works/README.md),
[Diagrams](../boxes/viewer/diagrams/README.md).
