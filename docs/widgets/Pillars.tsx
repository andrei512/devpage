// A grid of cards from a Markdown list: each item's bold text is the card's
// title, the rest its line; cards are numbered in order.
export const css = `
  .pillars-slide { position: absolute; inset: 0; padding: 96px 128px; display: flex; flex-direction: column; gap: 64px; }
  .pillars-kicker { font-size: 30px; font-weight: 600; letter-spacing: 0.14em; text-transform: uppercase; color: var(--accent); margin-bottom: -48px; }
  .pillars-title { font-size: 84px; font-weight: 700; letter-spacing: -0.03em; line-height: 1.05; margin: 0; }
  .pillars { flex: 1; display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 40px; align-content: center; }
  .pillar {
    padding: 44px 48px; border-radius: 32px; background: var(--surface); border: 1px solid var(--border);
    display: flex; flex-direction: column; gap: 14px;
  }
  .pillar-number { font-size: 28px; font-weight: 700; color: var(--accent); letter-spacing: 0.08em; }
  .pillar-title { font-size: 50px; font-weight: 700; letter-spacing: -0.02em; line-height: 1.1; }
  .pillar-text { font-size: 34px; color: var(--muted); line-height: 1.35; }
  .pillar-text code { font-size: 0.85em; }
`;

export default function Pillars({ title, kicker, html }: { title?: string; kicker?: string; html: string }) {
  const parsed = new DOMParser().parseFromString(html, "text/html");
  const items = Array.from(parsed.querySelectorAll("li"));
  return (
    <div className="pillars-slide">
      {kicker ? <div className="pillars-kicker">{kicker}</div> : null}
      {title ? <h2 className="pillars-title">{title}</h2> : null}
      <div className="pillars">
        {items.map((item, i) => {
          const strong = item.querySelector("strong");
          const head = strong?.textContent ?? "";
          strong?.remove();
          const text = item.innerHTML.replace(/^\s*(—|-|:)\s*/, "").trim();
          return (
            <div className="pillar">
              <div className="pillar-number">{String(i + 1).padStart(2, "0")}</div>
              <div className="pillar-title">{head}</div>
              <div className="pillar-text" dangerouslySetInnerHTML={{ __html: text }} />
            </div>
          );
        })}
      </div>
    </div>
  );
}
