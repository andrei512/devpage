// Four ways to share: a PDF, a video, a site for the team, and that site on
// a phone, its sidebar behind the button at the top left.
export const css = `
  .sh { display: grid; grid-template-columns: repeat(4, 400px); gap: 40px; }
  .sh-tile { background: var(--surface); border: 1px solid var(--border); border-radius: 28px; padding: 44px 40px 40px; display: flex; flex-direction: column; gap: 14px; }
  .sh-pic { height: 230px; display: grid; place-items: center; margin-bottom: 16px; }
  .sh-name { font-size: 56px; font-weight: 700; letter-spacing: -0.03em; }
  .sh-cmd { font-family: var(--font-mono); font-size: 28px; color: var(--muted); }
  .sk2 { background: var(--background); border: 1px solid var(--border); border-radius: 12px; box-shadow: 0 20px 40px -24px color-mix(in srgb, var(--text) 45%, transparent); }
  .sk2-line { height: 10px; border-radius: 5px; background: var(--border); }
`;

function Pdf() {
  return (
    <div className="sk2" style={{ width: "160px", height: "220px", padding: "22px", display: "grid", gap: "12px", alignContent: "start", position: "relative" }}>
      {[1, 2, 3, 4, 5].map((i) => <div className="sk2-line" style={{ width: i % 2 ? "100%" : "70%" }} />)}
      <div style={{ position: "absolute", right: "-22px", bottom: "22px", background: "#ff3b30", color: "white", fontWeight: 700, fontSize: "26px", padding: "6px 14px", borderRadius: "8px" }}>PDF</div>
    </div>
  );
}

function Video() {
  return (
    <div className="sk2" style={{ width: "320px", height: "180px", display: "grid", placeItems: "center" }}>
      <div style={{ width: "84px", height: "84px", borderRadius: "50%", background: "var(--accent)", display: "grid", placeItems: "center" }}>
        <div style={{ width: 0, height: 0, borderTop: "18px solid transparent", borderBottom: "18px solid transparent", borderLeft: "28px solid white", marginLeft: "8px" }} />
      </div>
    </div>
  );
}

function Team() {
  return (
    <div className="sk2" style={{ width: "320px", height: "200px", overflow: "hidden" }}>
      <div style={{ height: "40px", background: "var(--surface)", borderBottom: "1px solid var(--border)", display: "flex", alignItems: "center", gap: "10px", padding: "0 14px", fontSize: "18px", color: "var(--muted)" }}>
        <span>🔒</span><span style={{ fontFamily: "var(--font-mono)" }}>docs.your-team.com</span>
      </div>
      <div style={{ padding: "22px", display: "grid", gap: "12px" }}>
        <div className="sk2-line" style={{ width: "50%", background: "var(--accent)" }} /><div className="sk2-line" /><div className="sk2-line" style={{ width: "80%" }} />
      </div>
    </div>
  );
}

function Phone() {
  return (
    <div className="sk2" style={{ width: "124px", height: "224px", borderRadius: "24px", padding: "7px", boxSizing: "border-box" }}>
      <div style={{ height: "100%", borderRadius: "18px", border: "1px solid var(--border)", overflow: "hidden", display: "flex", flexDirection: "column" }}>
        <div style={{ height: "28px", background: "var(--surface)", borderBottom: "1px solid var(--border)", display: "flex", alignItems: "center", gap: "7px", padding: "0 9px" }}>
          <div style={{ width: "13px", height: "11px", border: "2px solid var(--muted)", borderRadius: "3px", boxSizing: "border-box", borderLeftWidth: "5px" }} />
          <div className="sk2-line" style={{ width: "40px", height: "6px" }} />
        </div>
        <div style={{ padding: "14px 10px", display: "grid", gap: "9px" }}>
          <div className="sk2-line" style={{ width: "70%", height: "9px", background: "var(--accent)" }} />
          {[1, 2, 3, 4, 5, 6].map((i) => <div className="sk2-line" style={{ width: i % 3 ? "100%" : "60%", height: "6px" }} />)}
        </div>
      </div>
    </div>
  );
}

const ways = [
  { name: "PDF", cmd: "devpage pdf", Pic: Pdf },
  { name: "Video", cmd: "devpage video", Pic: Video },
  { name: "Team site", cmd: "behind your login", Pic: Team },
  { name: "Phone", cmd: "the same link", Pic: Phone },
];

export default function ShareTiles() {
  return (
    <div className="sh">
      {ways.map(({ name, cmd, Pic }) => (
        <div className="sh-tile">
          <div className="sh-pic"><Pic /></div>
          <div className="sh-name">{name}</div>
          <div className="sh-cmd">{cmd}</div>
        </div>
      ))}
    </div>
  );
}
