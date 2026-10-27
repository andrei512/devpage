// This talk as files: the deck's folder, and the one command that turns it
// into a video.
import { Window, framesCss } from "./frames";

export const css = framesCss + `
  .tf { display: flex; align-items: center; gap: 56px; }
  .tf-list { font-family: var(--font-mono); font-size: 30px; line-height: 1.75; }
  .tf-list .dim { color: var(--muted); }
  .tf-cmd { font-family: var(--font-mono); font-size: 36px; line-height: 1.7; white-space: nowrap; }
  .tf-cmd .out { color: #98989d; }
`;

export default function TalkFiles() {
  return (
    <div className="tf">
      <Window title="docs/talks/devpage/" style={{ width: "620px" }}>
        <div className="tf-list">
          <div>devpage.yml</div>
          <div>title.md</div>
          <div>agents-write.md</div>
          <div>one-command.md</div>
          <div className="dim">… 9 more</div>
        </div>
      </Window>
      <span className="arrow">→</span>
      <Window title="my-app" dark style={{ width: "820px" }}>
        <div className="tf-cmd">$ devpage video docs/talks/devpage<div className="out">devpage.mp4</div></div>
      </Window>
    </div>
  );
}
