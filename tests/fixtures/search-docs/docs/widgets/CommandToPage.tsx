// One command on the left, the page it serves on the right.
import { Window, SyncPage, framesCss } from "./frames";

export const css = framesCss + `
  .ctp { display: flex; align-items: center; gap: 56px; }
  .ctp-cmd { font-family: var(--font-mono); font-size: 40px; line-height: 1.6; white-space: nowrap; }
  .ctp-cmd .out { color: #98989d; font-size: 30px; }
`;

export default function CommandToPage() {
  return (
    <div className="ctp">
      <Window title="my-app" dark style={{ width: "600px" }}>
        <div className="ctp-cmd">$ devpage docs<div className="out">→ localhost:3030</div></div>
      </Window>
      <span className="arrow">→</span>
      <Window title="localhost:3030/docs/design/sync.md" style={{ width: "880px" }}>
        <SyncPage />
      </Window>
    </div>
  );
}
