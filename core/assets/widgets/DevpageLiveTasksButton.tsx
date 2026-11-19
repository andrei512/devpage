// The button in the navbar that opens the tasks sidebar. It shows how many tasks are running.
import { panel, useLive } from "./live";

export const css = `
  .live-tasks-button { position: relative; display: inline-flex; align-items: center; justify-content: center; gap: 5px; min-width: 30px; height: 30px; padding: 0 7px; border: 0; border-radius: 8px; background: none; color: var(--muted); cursor: pointer; transition: color 0.15s, background 0.15s; }
  .live-tasks-button:hover { color: var(--text); background: var(--surface); }
  .live-tasks-button[aria-pressed="true"] { color: var(--accent); background: color-mix(in srgb, var(--accent) 14%, transparent); }
  .live-tasks-button b { min-width: 16px; height: 16px; padding: 0 5px; border-radius: 8px; background: #e64c3c; color: #fff; font-size: 10px; font-weight: 700; line-height: 16px; }
`;

export default function DevpageLiveTasksButton() {
  const live = useLive();
  if (!live.connected) return null;
  const running = live.tasks.filter((task) => task.state === "running").length;
  return (
    <button className="live-tasks-button" onClick={() => panel(!live.panel)} aria-pressed={live.panel} aria-label="Tasks" title="Tasks">
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
        <path d="M9 6h12M9 12h12M9 18h12M3.5 6h1M3.5 12h1M3.5 18h1" />
      </svg>
      {running > 0 && <b>{running}</b>}
    </button>
  );
}
