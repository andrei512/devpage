// Shared pieces for the talk's pictures: a window (an editor, a terminal, a
// browser) and a small rendered page, drawn with the theme's tokens.
export const framesCss = `
  .w { background: var(--background); border: 1px solid var(--border); border-radius: 22px; overflow: hidden; display: flex; flex-direction: column;
       box-shadow: 0 40px 80px -40px color-mix(in srgb, var(--text) 45%, transparent); text-align: left; }
  .w-bar { display: flex; align-items: center; gap: 14px; padding: 16px 22px; background: var(--surface); border-bottom: 1px solid var(--border); }
  .w-dots { display: flex; gap: 9px; }
  .w-dots i { display: block; width: 14px; height: 14px; border-radius: 50%; background: var(--border); }
  .w-title { flex: 1; text-align: center; margin-right: 66px; font-family: var(--font-mono); font-size: 22px; color: var(--muted); }
  .w-body { padding: 34px 40px; flex: 1; }
  .w.dark { background: #1d1d1f; border-color: #3a3a3c; color: #f5f5f7; }
  .w.dark .w-bar { background: #2c2c2e; border-color: #3a3a3c; }
  .w.dark .w-dots i { background: #48484a; }
  .w.dark .w-title { color: #98989d; }
  .mono { font-family: var(--font-mono); }
  .arrow { font-size: 96px; color: var(--accent); font-weight: 300; line-height: 1; }
  .page-h { font-size: 40px; font-weight: 700; letter-spacing: -0.02em; margin: 0 0 18px; }
  .page-p { font-size: 28px; line-height: 1.45; margin: 0 0 22px; color: var(--text); }
  .mini { display: flex; align-items: center; gap: 14px; margin: 8px 0 26px; }
  .mini b { font-weight: 500; font-size: 22px; padding: 14px 22px; border: 1px solid var(--border); border-radius: 12px; background: var(--surface); }
  .mini i { font-style: normal; color: var(--muted); font-size: 26px; }
  .mini-table { border-collapse: collapse; font-size: 22px; }
  .mini-table td, .mini-table th { border-bottom: 1px solid var(--border); padding: 10px 22px 10px 0; text-align: left; }
  .mini-table th { color: var(--muted); font-weight: 600; }
`;

export function Window({ title, dark, style, children }: { title?: string; dark?: boolean; style?: any; children?: any }) {
  return (
    <div className={dark ? "w dark" : "w"} style={style}>
      <div className="w-bar"><span className="w-dots"><i /><i /><i /></span><span className="w-title">{title}</span></div>
      <div className="w-body">{children}</div>
    </div>
  );
}

// The design page the talk keeps coming back to, rendered.
export function SyncPage() {
  return (
    <div>
      <h3 className="page-h">How a write reaches the server</h3>
      <p className="page-p">The app keeps every write in an <b>outbox</b>.</p>
      <div className="mini"><b>App</b><i>→</i><b>Outbox</b><i>→</i><b>Server</b></div>
      <table className="mini-table">
        <tr><th>Case</th><th>Retry</th><th>Limit</th></tr>
        <tr><td>offline</td><td>yes</td><td>24 h</td></tr>
      </table>
    </div>
  );
}
