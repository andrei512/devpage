// A site's frame drawn twice, light and dark: the navbar with the logo, links,
// an open dropdown and the theme switch, the sidebar, and the page.
export const css = `
  .frames { display: grid; grid-template-columns: 1fr 1fr; gap: 36px; }
  .frame-light { --background: #ffffff; --surface: #f5f5f7; --text: #1d1d1f; --muted: #6e6e73; --border: #d2d2d7; --accent: #0066cc; }
  .frame-dark { --background: #000000; --surface: #1d1d1f; --text: #f5f5f7; --muted: #a1a1a6; --border: #424245; --accent: #2997ff; }
  .frame {
    position: relative; height: 470px; border-radius: 20px; overflow: hidden;
    background: var(--background); color: var(--text); border: 1px solid var(--border);
    box-shadow: 0 24px 48px -28px rgba(0, 0, 0, 0.5);
  }
  .frame-bar { display: flex; align-items: center; gap: 14px; height: 64px; padding: 0 22px; border-bottom: 1px solid var(--border); font-size: 22px; }
  .frame-logo { width: 26px; height: 26px; }
  .frame-title { font-weight: 700; margin-right: auto; }
  .frame-link { color: var(--muted); }
  .frame-link.current { color: var(--text); font-weight: 600; }
  .frame-switch { width: 34px; height: 34px; border-radius: 50%; border: 1px solid var(--border); display: grid; place-items: center; font-size: 18px; }
  .frame-menu {
    position: absolute; top: 58px; right: 70px; width: 210px; padding: 10px; border-radius: 14px;
    background: var(--background); border: 1px solid var(--border); box-shadow: 0 16px 32px -16px rgba(0, 0, 0, 0.45);
    display: grid; gap: 6px; font-size: 18px; z-index: 1;
  }
  .frame-menu div { padding: 8px 10px; border-radius: 8px; }
  .frame-menu div:first-child { background: var(--surface); }
  .frame-body { display: grid; grid-template-columns: 150px 1fr; height: calc(100% - 64px); }
  .frame-side { background: var(--surface); border-right: 1px solid var(--border); padding: 20px 16px; display: grid; align-content: start; gap: 12px; }
  .frame-main { padding: 26px 30px; display: grid; align-content: start; gap: 14px; }
  .frame-line { height: 10px; border-radius: 5px; background: var(--border); }
  .frame-line.accent { background: var(--accent); width: 45%; height: 14px; }
  .frame-line.short { width: 60%; }
  .frame-caption { text-align: center; font-size: 26px; color: var(--muted); margin-top: 16px; }
`;

function Frame({ theme }: { theme: "light" | "dark" }) {
  return (
    <div>
      <div className={`frame frame-${theme}`}>
        <div className="frame-bar">
          <img className="frame-logo" src="/docs/logo.svg" alt="" />
          <span className="frame-title">devpage</span>
          <span className="frame-link current">Docs</span>
          <span className="frame-link">Examples ▾</span>
          <span className="frame-switch">{theme === "light" ? "☾" : "☀"}</span>
        </div>
        {theme === "dark" ? (
          <div className="frame-menu"><div>Site</div><div>Doc</div><div>Slides</div><div>Design</div></div>
        ) : null}
        <div className="frame-body">
          <div className="frame-side">
            <div className="frame-line accent" style={{ width: "70%" }} />
            <div className="frame-line" /><div className="frame-line short" /><div className="frame-line" /><div className="frame-line short" />
          </div>
          <div className="frame-main">
            <div className="frame-line accent" />
            <div className="frame-line" /><div className="frame-line" /><div className="frame-line short" />
            <div className="frame-line" /><div className="frame-line short" />
          </div>
        </div>
      </div>
      <div className="frame-caption">{theme === "light" ? "Light" : "Dark — one click"}</div>
    </div>
  );
}

export default function FrameSketch() {
  return (
    <div className="frames">
      <Frame theme="light" />
      <Frame theme="dark" />
    </div>
  );
}
