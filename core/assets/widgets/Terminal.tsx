// A terminal window: lines starting with "$ " are commands, the rest output.
export default function Terminal({ title, source }: { title?: string; source: string }) {
  const text = source.replace(/^```[^\n]*\n/, "").replace(/\n```\s*$/, "");
  return (
    <figure className="terminal">
      <figcaption>{title ?? "Terminal"}</figcaption>
      <pre>
        {text.split("\n").map((line) =>
          line.startsWith("$ ") ? (
            <div className="terminal-command"><span className="terminal-prompt">$ </span>{line.slice(2)}</div>
          ) : (
            <div className="terminal-output">{line}</div>
          ),
        )}
      </pre>
    </figure>
  );
}
