// A repository's files, most of them written by the agent this week.
import { Window, framesCss } from "./frames";

export const css = framesCss + `
  .af { width: 1180px; }
  .af-row { display: flex; align-items: center; gap: 24px; font-size: 38px; padding: 18px 0; border-bottom: 1px solid var(--border); }
  .af-row:last-child { border-bottom: 0; }
  .af-row svg { width: 40px; height: 40px; flex: none; color: var(--muted); }
  .af-name { flex: 1; font-family: var(--font-mono); font-size: 34px; }
  .af-tag { font-size: 24px; font-weight: 600; color: var(--accent); background: color-mix(in srgb, var(--accent) 12%, transparent); padding: 8px 18px; border-radius: 999px; }
  .af-row.code .af-name { color: var(--muted); }
`;

const files = [
  { name: "docs/design/sync.md", by: "design" },
  { name: "docs/plan/01-outbox.md", by: "plan" },
  { name: "docs/plan/02-retry.md", by: "plan" },
  { name: "docs/review/outbox.md", by: "review" },
  { name: "src/sync.rs", by: "" },
];

function Page() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round">
      <path d="M6 2.5h8l4.5 4.5v14.5H6z" /><path d="M14 2.5V7h4.5" />
    </svg>
  );
}

export default function AgentFiles() {
  return (
    <div className="af">
      <Window title="my-app">
        {files.map((f) => (
          <div className={f.by ? "af-row" : "af-row code"}>
            <Page />
            <span className="af-name">{f.name}</span>
            {f.by ? <span className="af-tag">written by your agent</span> : null}
          </div>
        ))}
      </Window>
    </div>
  );
}
