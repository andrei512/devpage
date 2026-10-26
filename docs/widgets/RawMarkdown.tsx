// What a design looks like before devpage: raw Markdown, a diagram as code,
// a table as pipes.
import { Window, framesCss } from "./frames";

export const css = framesCss + `
  .raw { width: 1300px; }
  .raw pre { background: none !important; border: 0 !important; padding: 0 !important; border-radius: 0; box-shadow: none; overflow: visible; margin: 0; font-family: var(--font-mono); font-size: 32px; line-height: 1.5; color: #e5e5ea; white-space: pre; }
  .raw .m { color: #8e8e93; }
`;

const lines: [string, string][] = [
  ["## ", "How a write reaches the server"],
  ["", "The app keeps every write in an **outbox**."],
  ["```mermaid", ""],
  ["", "flowchart LR"],
  ["", "  app[App] --> outbox[(Outbox)] -->|retry| server[Server]"],
  ["```", ""],
  ["| ", "Case    | Retry | Limit |"],
  ["|", "---------|-------|-------|"],
  ["| ", "offline | yes   | 24 h  |"],
];

export default function RawMarkdown() {
  return (
    <div className="raw">
      <Window title="docs/design/sync.md" dark>
        <pre>{lines.map(([mark, text]) => <div><span className="m">{mark}</span>{text || " "}</div>)}</pre>
      </Window>
    </div>
  );
}
