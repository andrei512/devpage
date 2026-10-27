// A comment on a sentence, and what happens to it: the agent reads it,
// fixes the text, and closes it.
import { Window, framesCss } from "./frames";

export const css = framesCss + `
  .cl { display: flex; flex-direction: column; align-items: center; gap: 56px; }
  .cl-top { display: flex; align-items: center; gap: 40px; }
  .cl p { font-size: 34px; line-height: 1.5; margin: 0; }
  .cl mark { background: color-mix(in srgb, #ffcc00 40%, transparent); color: inherit; border-radius: 6px; padding: 0 4px; }
  .cl-card { width: 520px; background: var(--background); border: 1px solid var(--border); border-radius: 22px; padding: 28px 32px;
             box-shadow: 0 40px 80px -40px color-mix(in srgb, var(--text) 45%, transparent); }
  .cl-who { display: flex; align-items: center; gap: 16px; font-size: 28px; font-weight: 600; margin-bottom: 14px; }
  .cl-who i { width: 52px; height: 52px; border-radius: 50%; background: #ff9f0a; color: white; display: grid; place-items: center; font-style: normal; font-size: 26px; }
  .cl-card div:last-child { font-size: 32px; line-height: 1.4; }
  .cl-steps { display: flex; align-items: center; gap: 28px; font-size: 32px; color: var(--muted); }
  .cl-steps span { padding: 14px 28px; border-radius: 999px; background: var(--surface); border: 1px solid var(--border); color: var(--text); }
  .cl-steps span.done { color: #248a3d; background: color-mix(in srgb, #34c759 16%, transparent); border-color: transparent; }
`;

export default function CommentLoop() {
  return (
    <div className="cl">
      <div className="cl-top">
        <Window title="localhost:3030/docs/design/sync.md" style={{ width: "900px" }}>
          <p>The app keeps <mark>every write</mark> in an outbox and retries for 24 hours.</p>
        </Window>
        <div className="cl-card">
          <div className="cl-who"><i>A</i>Ana</div>
          <div>Every write, or only the failed ones?</div>
        </div>
      </div>
      <div className="cl-steps">
        <span>Your agent reads it</span>→<span>fixes the text</span>→<span className="done">✓ comment closed</span>
      </div>
    </div>
  );
}
