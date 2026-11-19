// The live session, started: the reader has clicked one sentence of the page
// and says what they want. The round button in the page's corner is the
// session's, in the user's colour while they speak.
import { Window, framesCss } from "./frames";

export const talkCss = `
  .tk { --user: #e64c3c; }
  .tk h3 { font-size: 42px; margin: 0 0 18px; letter-spacing: -0.02em; }
  .tk p { font-size: 32px; line-height: 1.5; margin: 0 0 14px; }
  .tk-mark { background: color-mix(in srgb, var(--user) 26%, transparent); border-radius: 8px; padding: 6px 12px; margin-left: -12px; margin-right: -12px; }
  .tk-orb { display: block; width: 84px; height: 84px; border-radius: 50%; background: var(--user);
            box-shadow: 0 0 0 5px var(--background), 0 0 0 8px var(--user), 0 0 0 22px color-mix(in srgb, var(--user) 16%, transparent); }
`;

export function Orb() {
  return <span className="tk-orb" />;
}

export const css = framesCss + talkCss + `
  .tp { display: flex; align-items: center; gap: 64px; }
  .tp-foot { display: flex; justify-content: flex-end; padding: 30px 14px 14px 0; }
  .tp-said { width: 560px; text-align: left; }
  .tp-who { font-size: 26px; color: var(--muted); font-weight: 600; margin: 0 0 14px 6px; }
  .tp-line { font-size: 52px; font-weight: 600; line-height: 1.25; letter-spacing: -0.02em; }
`;

export default function TalkPoint() {
  return (
    <div className="tp tk">
      <Window title="localhost:3030/docs/design/sync.md" style={{ width: "980px" }}>
        <h3>Retries</h3>
        <p>The app keeps every write in an outbox.</p>
        <p className="tk-mark">It retries a failed write for 24 hours.</p>
        <p>It stops when you sign out.</p>
        <div className="tp-foot"><Orb /></div>
      </Window>
      <div className="tp-said">
        <div className="tp-who">You say</div>
        <div className="tp-line">“Make this two days.”</div>
      </div>
    </div>
  );
}
