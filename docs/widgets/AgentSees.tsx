// The agent looks at a real slide of this talk, as an image, and fixes it.
// `src` is the image; `title` the address it came from, e.g. devpage.yml/5.png.
import { Window, framesCss } from "./frames";

export const css = framesCss + `
  .as { display: flex; align-items: center; gap: 64px; }
  .as img { display: block; width: 100%; border-radius: 10px; border: 1px solid var(--border); }
  .as-chat { display: flex; flex-direction: column; gap: 22px; width: 620px; }
  .as-msg { font-size: 34px; line-height: 1.4; padding: 22px 30px; border-radius: 28px; background: var(--surface); border: 1px solid var(--border); }
  .as-msg.agent { background: var(--accent); color: white; border-color: transparent; }
  .as-who { font-size: 24px; color: var(--muted); font-weight: 600; margin: 0 0 -10px 12px; }
`;

export default function AgentSees({ src, title }: { src: string; title?: string }) {
  return (
    <div className="as">
      <Window title={title || src} style={{ width: "880px" }}>
        <img src={src} alt="A slide of this talk" />
      </Window>
      <div className="as-chat">
        <div className="as-who">Your agent</div>
        <div className="as-msg agent">I opened slide 5 as an image.</div>
        <div className="as-msg agent">The arrow is too small. I made it bigger.</div>
      </div>
    </div>
  );
}
