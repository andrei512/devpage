// Keys as keycaps, inline: <Keys>⌘ P</Keys>.
export default function Keys({ source }: { source: string }) {
  return (
    <span className="keys">
      {source.trim().split(/\s+/).map((key) => <kbd>{key}</kbd>)}
    </span>
  );
}
