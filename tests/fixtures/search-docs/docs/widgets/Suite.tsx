// The four kinds of document as an office suite: what each is, and the
// office app it stands in for.
export const css = `
  .suite { display: grid; grid-template-columns: repeat(4, 360px); gap: 40px; }
  .suite-tile { background: var(--surface); border: 1px solid var(--border); border-radius: 28px; padding: 40px 36px 36px; display: flex; flex-direction: column; gap: 14px; }
  .suite-pic { height: 210px; display: grid; place-items: center; margin-bottom: 18px; }
  .suite-name { font-size: 56px; font-weight: 700; letter-spacing: -0.03em; }
  .suite-like { font-size: 32px; color: var(--muted); }
  .sk { background: var(--background); border: 1px solid var(--border); border-radius: 12px; box-shadow: 0 20px 40px -24px color-mix(in srgb, var(--text) 45%, transparent); }
  .sk-line { height: 10px; border-radius: 5px; background: var(--border); }
  .sk-line.a { background: var(--accent); }
`;

function Site() {
  return (
    <div className="sk" style={{ width: "250px", height: "170px", display: "flex", overflow: "hidden" }}>
      <div style={{ width: "64px", background: "var(--surface)", borderRight: "1px solid var(--border)", padding: "16px 10px", display: "grid", gap: "10px", alignContent: "start" }}>
        <div className="sk-line a" /><div className="sk-line" /><div className="sk-line" />
      </div>
      <div style={{ flex: 1, padding: "18px", display: "grid", gap: "12px", alignContent: "start" }}>
        <div className="sk-line a" style={{ width: "60%" }} /><div className="sk-line" /><div className="sk-line" style={{ width: "75%" }} /><div className="sk-line" />
      </div>
    </div>
  );
}

function Doc() {
  return (
    <div className="sk" style={{ width: "150px", height: "200px", padding: "20px", display: "grid", gap: "11px", alignContent: "start" }}>
      <div className="sk-line a" style={{ width: "60%" }} />
      {[1, 2, 3, 4, 5, 6].map((i) => <div className="sk-line" style={{ width: i % 3 ? "100%" : "70%" }} />)}
    </div>
  );
}

function Deck() {
  return (
    <div style={{ position: "relative", width: "250px", height: "160px" }}>
      <div className="sk" style={{ position: "absolute", width: "220px", height: "124px", left: "30px", top: "0" }} />
      <div className="sk" style={{ position: "absolute", width: "220px", height: "124px", left: "0", top: "30px", display: "grid", placeItems: "center" }}>
        <div className="sk-line a" style={{ width: "110px", height: "14px" }} />
      </div>
    </div>
  );
}

function Design() {
  return (
    <div className="sk" style={{ width: "110px", height: "200px", borderRadius: "20px", padding: "16px 12px", display: "flex", flexDirection: "column", gap: "10px" }}>
      <div className="sk-line a" style={{ width: "50%" }} /><div className="sk-line" /><div className="sk-line" style={{ width: "70%" }} />
      <div style={{ marginTop: "auto", height: "26px", borderRadius: "8px", background: "var(--accent)" }} />
    </div>
  );
}

const kinds = [
  { name: "Site", like: "like a website", Pic: Site },
  { name: "Doc", like: "like Word", Pic: Doc },
  { name: "Slides", like: "like PowerPoint", Pic: Deck },
  { name: "Design", like: "like Figma", Pic: Design },
];

export default function Suite() {
  return (
    <div className="suite">
      {kinds.map(({ name, like, Pic }) => (
        <div className="suite-tile">
          <div className="suite-pic"><Pic /></div>
          <div className="suite-name">{name}</div>
          <div className="suite-like">{like}</div>
        </div>
      ))}
    </div>
  );
}
