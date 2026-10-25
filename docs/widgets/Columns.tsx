// Side-by-side columns on a slide: each top-level block of the content is one
// column — or, with `rows`, one row. `title` heads the slide, `kicker` sits
// above it.
export const css = `
  .columns-slide { position: absolute; inset: 0; padding: 96px 128px; display: flex; flex-direction: column; gap: 56px; }
  .columns-kicker { font-size: 30px; font-weight: 600; letter-spacing: 0.14em; text-transform: uppercase; color: var(--accent); margin-bottom: -40px; }
  .columns-title { font-size: 84px; font-weight: 700; letter-spacing: -0.03em; line-height: 1.05; margin: 0; }
  .columns-grid { flex: 1; display: grid; grid-auto-flow: column; grid-auto-columns: minmax(0, 1fr); gap: 72px; align-items: center; }
  .columns-grid.rows { grid-auto-flow: row; grid-auto-rows: auto; align-content: center; gap: 48px; }
  .columns-grid > * { margin: 0; min-width: 0; }
  .columns-grid > p { font-size: 44px; line-height: 1.4; color: var(--muted); }
  .columns-grid > p strong { color: var(--text); }
`;

export default function Columns({ title, kicker, rows, children }: { title?: string; kicker?: string; rows?: boolean; children?: any }) {
  return (
    <div className="columns-slide">
      {kicker ? <div className="columns-kicker">{kicker}</div> : null}
      {title ? <h2 className="columns-title">{title}</h2> : null}
      <div className={rows ? "columns-grid rows" : "columns-grid"}>{children}</div>
    </div>
  );
}
