<Columns kicker="Your own widgets" title="Your own components: one file each, no build step.">

<FileTree>

- docs/widgets/
  - Chips.tsx — `<Chips>`
  - Columns.tsx
  - FrameSketch.tsx
  - Hero.tsx
  - KindCards.tsx
  - PaperSketch.tsx
  - Scattered.tsx

</FileTree>

```tsx
// Chips.tsx
export default
function Chips({ html }) {
  const items = parse(html);
  return (
    <ul className="chips">
      {items}
    </ul>
  );
}
```

<Chips>

- Callout
- Tabs
- Example
- FileTree
- Terminal
- Keys
- Diagram

</Chips>

</Columns>
