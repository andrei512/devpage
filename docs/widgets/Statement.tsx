// One big sentence on a slide, with an optional kicker above and Markdown
// content — usually a short list — under it.
export const css = `
  .statement {
    position: absolute; inset: 0; padding: 0 180px;
    display: flex; flex-direction: column; justify-content: center; gap: 40px;
  }
  .statement-kicker { font-size: 32px; font-weight: 600; letter-spacing: 0.14em; text-transform: uppercase; color: var(--accent); }
  .statement-text { font-size: 112px; font-weight: 700; letter-spacing: -0.035em; line-height: 1.05; margin: 0; }
  .statement-text em { font-style: normal; color: var(--accent); }
  .statement-body { font-size: 46px; color: var(--muted); line-height: 1.35; max-width: 1450px; }
  .statement-body p { margin: 0; }
  .statement-body ul { list-style: none; padding: 0; margin: 12px 0 0; display: flex; gap: 28px; flex-wrap: wrap; }
  .statement-body li {
    font-size: 38px; color: var(--text); padding: 14px 32px; border-radius: 999px;
    background: var(--surface); border: 1px solid var(--border);
  }
`;

export default function Statement({ text, kicker, children }: { text: string; kicker?: string; children?: any }) {
  return (
    <div className="statement">
      {kicker ? <div className="statement-kicker">{kicker}</div> : null}
      <h2 className="statement-text" dangerouslySetInnerHTML={{ __html: text.replace(/\*(.+?)\*/g, "<em>$1</em>") }} />
      {children ? <div className="statement-body">{children}</div> : null}
    </div>
  );
}
