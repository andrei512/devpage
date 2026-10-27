// One publish on the left, the link it answers with on the right: the page
// at that link, its latest version, as a reader opens it.
import { Window, SyncPage, framesCss } from "./frames";

export const css = framesCss + `
  .pl { display: flex; align-items: center; gap: 56px; }
  .pl-cmd { font-family: var(--font-mono); font-size: 32px; line-height: 1.6; white-space: nowrap; }
  .pl-cmd .out { color: #98989d; font-size: 23px; }
  .pl-cmd .gap { height: 28px; }
  .pl-version { display: inline-block; margin-bottom: 22px; font-size: 22px; font-weight: 600; padding: 8px 16px; border-radius: 999px; background: var(--surface); border: 1px solid var(--border); color: var(--muted); }
`;

export default function PublishLink() {
  return (
    <div className="pl">
      <Window title="design" dark style={{ width: "740px" }}>
        <div className="pl-cmd">
          $ devpage publish sync.md
          <div className="out">How a write reaches the server — version 1</div>
          <div className="out">https://docs.your-team.com/d/6f1c2a9e…</div>
          <div className="gap" />
          $ devpage publish sync.md
          <div className="out">How a write reaches the server — version 2</div>
          <div className="out">https://docs.your-team.com/d/6f1c2a9e…</div>
        </div>
      </Window>
      <span className="arrow">→</span>
      <Window title="docs.your-team.com/d/6f1c2a9e" style={{ width: "800px" }}>
        <span className="pl-version">Version 2 · latest</span>
        <SyncPage />
      </Window>
    </div>
  );
}
