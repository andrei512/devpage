// A talk slide: one big headline, centered, an optional line under it, and
// the slide's one picture filling the rest of the stage. `accent` is a second
// part of the headline in the accent color.
export const css = `
  .s { position: absolute; inset: 0; padding: 104px 140px 88px; display: flex; flex-direction: column; align-items: center; }
  .s-title { font-size: 112px; font-weight: 700; letter-spacing: -0.04em; line-height: 1.04; margin: 0; text-align: center; }
  .s-title em { font-style: normal; color: var(--accent); }
  .s-sub { font-size: 46px; line-height: 1.3; color: var(--muted); margin: 28px 0 0; text-align: center; }
  .s-stage { flex: 1; width: 100%; min-height: 0; margin-top: 72px; display: flex; align-items: center; justify-content: center; gap: 80px; }
  .s-stage > * { margin: 0; }
  .s-stage .diagram { width: 100%; }
`;

export default function Slide({ title, accent, sub, children }: { title: string; accent?: string; sub?: string; children?: any }) {
  return (
    <div className="s">
      <h2 className="s-title">{title}{accent ? <em> {accent}</em> : null}</h2>
      {sub ? <p className="s-sub">{sub}</p> : null}
      <div className="s-stage">{children}</div>
    </div>
  );
}
