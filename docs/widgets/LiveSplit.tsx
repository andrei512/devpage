// The agent saves a line; the open page shows it at once.
import { Window, framesCss } from "./frames";

export const css = framesCss + `
  .ls { display: flex; align-items: center; gap: 56px; }
  .ls pre { background: none !important; border: 0 !important; padding: 0 !important; border-radius: 0; box-shadow: none; overflow: visible; margin: 0; font-family: var(--font-mono); font-size: 30px; line-height: 1.6; white-space: pre-wrap; }
  .ls .new { background: color-mix(in srgb, var(--accent) 16%, transparent); border-radius: 8px; padding: 2px 6px; margin: 0 -6px; }
  .ls .caret { display: inline-block; width: 4px; height: 36px; background: var(--accent); vertical-align: -6px; margin-left: 4px; }
  .ls-pill { display: inline-block; margin-top: 26px; font-size: 24px; font-weight: 600; padding: 8px 20px; border-radius: 999px; }
  .ls-pill.saved { color: #248a3d; background: color-mix(in srgb, #34c759 18%, transparent); }
  .ls-pill.live { color: var(--accent); background: color-mix(in srgb, var(--accent) 12%, transparent); }
  .ls h3 { font-size: 42px; margin: 0 0 18px; letter-spacing: -0.02em; }
  .ls p { font-size: 30px; line-height: 1.45; margin: 0 0 16px; }
  .ls p.new { display: block; padding: 10px 14px; margin: 0 -14px; box-shadow: 0 0 0 2px var(--accent); }
`;

export default function LiveSplit() {
  return (
    <div className="ls">
      <Window title="sync.md — your agent" style={{ width: "760px" }}>
        <pre>{"## Retries\nThe app retries for 24 hours.\n"}<span className="new">It stops when you sign out.</span><span className="caret" /></pre>
        <span className="ls-pill saved">Saved</span>
      </Window>
      <span className="arrow">→</span>
      <Window title="localhost:3030" style={{ width: "760px" }}>
        <h3>Retries</h3>
        <p>The app retries for 24 hours.</p>
        <p className="new">It stops when you sign out.</p>
        <span className="ls-pill live">Updated, no reload</span>
      </Window>
    </div>
  );
}
