// Where a project's documents usually live: windows of four different tools,
// scattered and tilted, none of them the repository.
export const css = `
  .scattered-slide { position: absolute; inset: 0; padding: 104px 140px; }
  .scattered-kicker { font-size: 30px; font-weight: 600; letter-spacing: 0.14em; text-transform: uppercase; color: var(--accent); }
  .scattered-title { font-size: 112px; font-weight: 700; letter-spacing: -0.04em; line-height: 1.04; margin: 0; text-align: center; }
  .scattered-title em { font-style: normal; color: var(--accent); }
  .scattered-desk { position: absolute; left: 140px; right: 140px; top: 440px; bottom: 80px; }
  .tool-stale { position: absolute; right: 18px; bottom: 18px; font-size: 26px; font-weight: 700; color: #ff3b30; border: 3px solid #ff3b30; border-radius: 10px; padding: 4px 14px; background: var(--background); transform: rotate(8deg); }
  .tool {
    position: absolute; width: 400px; border-radius: 22px; overflow: hidden;
    background: var(--background); border: 1px solid var(--border);
    box-shadow: 0 30px 60px -30px color-mix(in srgb, var(--text) 45%, transparent);
  }
  .tool-bar { display: flex; align-items: center; gap: 12px; padding: 16px 22px; background: var(--surface); border-bottom: 1px solid var(--border); font-size: 26px; font-weight: 600; }
  .tool-dot { width: 14px; height: 14px; border-radius: 50%; }
  .tool-body { padding: 28px 28px 34px; display: grid; gap: 18px; min-height: 200px; align-content: start; }
  .tool-line { height: 12px; border-radius: 6px; background: var(--border); }
  .tool-line.accent { width: 50%; height: 16px; }
  .tool-line.short { width: 65%; }
  .tool-bubble { height: 40px; border-radius: 17px; background: var(--surface); border: 1px solid var(--border); width: 70%; }
  .tool-bubble.mine { margin-left: auto; width: 55%; background: color-mix(in srgb, #34c759 18%, var(--background)); }
  .tool-slide { aspect-ratio: 16 / 9; border-radius: 10px; border: 1px solid var(--border); display: grid; place-items: center; }
`;

const tools = [
  { name: "Wiki", color: "#8e8e93", left: "0px", top: "60px", turn: "-5deg", body: "page", stale: true },
  { name: "Slides", color: "#ff9f0a", left: "420px", top: "200px", turn: "4deg", body: "slide" },
  { name: "Docs", color: "#0a84ff", left: "840px", top: "30px", turn: "-3deg", body: "page", stale: true },
  { name: "Chat", color: "#34c759", left: "1260px", top: "180px", turn: "5deg", body: "chat" },
];

function Body({ kind }: { kind: string }) {
  if (kind === "chat") {
    return <div className="tool-body"><div className="tool-bubble" /><div className="tool-bubble mine" /><div className="tool-bubble" /></div>;
  }
  if (kind === "slide") {
    return <div className="tool-body"><div className="tool-slide"><div className="tool-line accent" style={{ width: "160px" }} /></div></div>;
  }
  return (
    <div className="tool-body">
      <div className="tool-line accent" style={{ background: "var(--muted)" }} />
      <div className="tool-line" /><div className="tool-line short" /><div className="tool-line" />
    </div>
  );
}

export default function Scattered({ kicker, title }: { kicker?: string; title: string }) {
  return (
    <div className="scattered-slide">
      {kicker ? <div className="scattered-kicker">{kicker}</div> : null}
      <h2 className="scattered-title" dangerouslySetInnerHTML={{ __html: title.replace(/\*(.+?)\*/g, "<em>$1</em>") }} />
      <div className="scattered-desk" aria-hidden="true">
        {tools.map((tool) => (
          <div className="tool" style={{ left: tool.left, top: tool.top, transform: `rotate(${tool.turn})` }}>
            <div className="tool-bar"><span className="tool-dot" style={{ background: tool.color }} />{tool.name}</div>
            <Body kind={tool.body} />
            {tool.stale ? <div className="tool-stale">out of date</div> : null}
          </div>
        ))}
      </div>
    </div>
  );
}
