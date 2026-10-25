// What `devpage pdf` hands back: a doc's A4 pages with header and footer, and
// a deck's 16:9 pages — drawn with the theme's tokens.
export const css = `
  .paper { display: flex; gap: 72px; align-items: flex-end; justify-content: center; }
  .paper-group { display: flex; flex-direction: column; align-items: center; gap: 24px; }
  .paper-label { font-size: 30px; color: var(--muted); font-weight: 600; letter-spacing: 0.06em; text-transform: uppercase; }
  .paper-sheet {
    background: var(--background); border: 1px solid var(--border); border-radius: 10px;
    box-shadow: 0 18px 40px -18px color-mix(in srgb, var(--text) 45%, transparent);
    display: flex; flex-direction: column; gap: 14px; padding: 26px 28px;
  }
  .paper-a4 { width: 300px; height: 424px; }
  .paper-169 { width: 420px; height: 236px; justify-content: center; align-items: center; }
  .paper-line { height: 9px; border-radius: 5px; background: var(--border); }
  .paper-line.accent { background: var(--accent); width: 55%; }
  .paper-line.short { width: 70%; }
  .paper-head, .paper-foot { font-size: 15px; color: var(--muted); display: flex; justify-content: space-between; }
  .paper-foot { margin-top: auto; }
  .paper-169 .paper-line { width: 60%; }
  .paper-169 .paper-line.accent { width: 40%; height: 14px; }
`;

function A4({ page }: { page: number }) {
  return (
    <div className="paper-sheet paper-a4">
      <div className="paper-head"><span>Offline sync — draft</span></div>
      <div className="paper-line accent" />
      {[0, 1, 2, 3, 4, 5, 6].map((i) => <div className={`paper-line ${i % 3 === 2 ? "short" : ""}`} />)}
      <div className="paper-foot"><span /><span>Page {page} of 3</span></div>
    </div>
  );
}

function Slide() {
  return (
    <div className="paper-sheet paper-169">
      <div className="paper-line accent" />
      <div className="paper-line" />
      <div className="paper-line short" />
    </div>
  );
}

export default function PaperSketch() {
  return (
    <div className="paper">
      <div className="paper-group">
        <A4 page={1} />
        <div className="paper-label">A doc, on A4</div>
      </div>
      <div className="paper-group">
        <div style={{ display: "flex", flexDirection: "column", gap: "18px" }}>
          <Slide />
          <Slide />
        </div>
        <div className="paper-label">A deck, one slide a page</div>
      </div>
    </div>
  );
}
