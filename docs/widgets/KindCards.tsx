// The four kinds of document, side by side: a sketch of how each reads, what
// it is made of, and a link to the page that teaches it.
const kinds = [
  {
    name: "Site",
    kind: "site",
    like: "Like a docs website",
    madeOf: "A root page and the pages its Topics sections reach",
    readAs: "Many pages, with a sidebar",
    href: "/docs/boxes/viewer/making-a-site/README.md",
  },
  {
    name: "Doc",
    kind: "doc",
    like: "Like a Word document",
    madeOf: "One or more chapter files, in order",
    readAs: "One long page, with an outline that follows you",
    href: "/docs/boxes/viewer/writing-a-doc/README.md",
  },
  {
    name: "Slides",
    kind: "slides",
    like: "Like a PowerPoint deck",
    madeOf: "One Markdown file per slide, in order",
    readAs: "One slide at a time, 16:9",
    href: "/docs/boxes/viewer/making-slides/README.md",
  },
  {
    name: "Design",
    kind: "design",
    like: "Like a Figma frame and its prototype",
    madeOf: "One .design file, using other designs",
    readAs: "A canvas you can click through",
    href: "/docs/boxes/viewer/designs/README.md",
    // A design has no document file: the .design file is the whole document.
    field: "checkout.design",
  },
];

export const css = `
  .kind-cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(16rem, 1fr));
    gap: 1.25rem;
    margin: 1.75rem 0 2rem;
  }
  /* On a slide, the four side by side. */
  .slide .kind-cards { grid-template-columns: repeat(4, minmax(0, 1fr)); }
  .slide .kind-sketch { height: 7.5rem; }
  .slide .kind-sketch > * { zoom: 0.8; }
  .slide .kind-body { padding-bottom: 1rem; }
  .kind-cards a.kind-card {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border);
    border-radius: 18px;
    background: var(--background);
    color: inherit;
    text-decoration: none;
    overflow: hidden;
    transition: transform 0.2s ease, box-shadow 0.2s ease, border-color 0.2s ease;
  }
  .kind-cards a.kind-card:hover {
    transform: translateY(-3px);
    border-color: color-mix(in srgb, var(--accent) 45%, var(--border));
    box-shadow: 0 12px 32px -12px color-mix(in srgb, var(--text) 30%, transparent);
  }

  /* The sketch: a tiny, theme-colored picture of how the kind reads. */
  .kind-sketch {
    height: 9.5rem;
    background: var(--surface);
    border-bottom: 1px solid var(--border);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .kind-sketch .frame {
    background: var(--background);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: 0 6px 18px -10px color-mix(in srgb, var(--text) 40%, transparent);
    transition: transform 0.25s ease;
  }
  .kind-card:hover .kind-sketch .frame { transform: scale(1.04); }
  .kind-sketch .bar { height: 5px; border-radius: 3px; background: var(--border); }
  .kind-sketch .bar.accent { background: var(--accent); opacity: 0.85; }
  .kind-sketch .bar.short { width: 55%; }
  .kind-sketch .bar.mid { width: 75%; }

  .kind-sketch .site { width: 11rem; height: 6.75rem; display: grid; grid-template-columns: 3rem 1fr; overflow: hidden; }
  .kind-sketch .site .side { background: var(--surface); border-right: 1px solid var(--border); padding: 0.5rem 0.4rem; display: grid; align-content: start; gap: 0.35rem; }
  .kind-sketch .site .main { padding: 0.55rem 0.6rem; display: grid; align-content: start; gap: 0.35rem; }

  .kind-sketch .doc { width: 11rem; height: 6.75rem; display: grid; grid-template-columns: 2.6rem 1fr; gap: 0.5rem; padding: 0.55rem 0.6rem; }
  .kind-sketch .doc .outline { display: grid; align-content: start; gap: 0.35rem; padding-right: 0.45rem; border-right: 1px solid var(--border); }
  .kind-sketch .doc .text { display: grid; align-content: start; gap: 0.35rem; }

  .kind-sketch .deck { position: relative; width: 10.5rem; height: 6.4rem; }
  .kind-sketch .deck .frame { position: absolute; width: 9rem; aspect-ratio: 16 / 9; height: auto; }
  .kind-sketch .deck .back2 { left: 1.5rem; top: 0; opacity: 0.45; }
  .kind-sketch .deck .back1 { left: 0.75rem; top: 0.55rem; opacity: 0.75; }
  .kind-sketch .deck .front { left: 0; top: 1.1rem; display: grid; align-content: center; justify-items: center; gap: 0.4rem; }
  .kind-sketch .deck .front .bar { width: 55%; }
  .kind-sketch .deck .front .bar.title { height: 8px; width: 45%; }

  .kind-sketch .design { width: 4.6rem; height: 7.4rem; border-radius: 12px; padding: 0.6rem 0.5rem; display: grid; align-content: start; gap: 0.4rem; }
  .kind-sketch .design .card { height: 1.6rem; border: 1px solid var(--border); border-radius: 5px; }
  .kind-sketch .design .button { height: 0.85rem; margin-top: 0.9rem; border-radius: 4px; background: var(--accent); opacity: 0.85; }

  .kind-body { display: flex; flex-direction: column; gap: 0.35rem; padding: 1.1rem 1.25rem 1.25rem; flex: 1; }
  .kind-title { display: flex; align-items: center; justify-content: space-between; gap: 0.75rem; }
  .kind-title strong { font-size: 1.3rem; font-weight: 650; letter-spacing: -0.01em; }
  .kind-title .arrow { color: var(--accent); opacity: 0; transform: translateX(-4px); transition: opacity 0.2s ease, transform 0.2s ease; }
  .kind-card:hover .kind-title .arrow { opacity: 1; transform: none; }
  .kind-like { color: var(--muted); font-size: 0.95rem; }
  .kind-facts { margin: 0.75rem 0 0; padding-top: 0.75rem; border-top: 1px solid var(--border); display: grid; gap: 0.55rem; font-size: 0.9rem; line-height: 1.4; flex: 1; }
  .kind-facts dt { font-size: 0.7rem; font-weight: 600; letter-spacing: 0.06em; text-transform: uppercase; color: var(--muted); }
  .kind-facts dd { margin: 0.1rem 0 0; }
  .kind-field { align-self: flex-start; margin-top: 0.9rem; font-family: var(--font-mono); font-size: 0.75rem; color: var(--muted); background: var(--surface); border: 1px solid var(--border); border-radius: 999px; padding: 0.15rem 0.6rem; }
`;

function Bars({ count, accent }: { count: number; accent?: boolean }) {
  const widths = ["", "mid", "", "short", "mid", ""];
  return (
    <>
      {accent && <div className="bar accent short" />}
      {Array.from({ length: count }, (_, i) => <div className={`bar ${widths[i % widths.length]}`} />)}
    </>
  );
}

function Sketch({ kind }: { kind: string }) {
  if (kind === "site") {
    return (
      <div className="frame site">
        <div className="side"><div className="bar accent" /><Bars count={4} /></div>
        <div className="main"><Bars count={6} accent /></div>
      </div>
    );
  }
  if (kind === "doc") {
    return (
      <div className="frame doc">
        <div className="outline"><div className="bar accent" /><Bars count={3} /></div>
        <div className="text"><Bars count={7} accent /></div>
      </div>
    );
  }
  if (kind === "design") {
    return (
      <div className="frame design">
        <div className="bar accent mid" />
        <div className="card" />
        <div className="bar short" />
        <div className="button" />
      </div>
    );
  }
  return (
    <div className="deck">
      <div className="frame back2" />
      <div className="frame back1" />
      <div className="frame front"><div className="bar accent title" /><div className="bar" /><div className="bar short" /></div>
    </div>
  );
}

export default function KindCards() {
  return (
    <div className="kind-cards">
      {kinds.map((kind) => (
        <a className="kind-card" href={kind.href}>
          <div className="kind-sketch" aria-hidden="true"><Sketch kind={kind.kind} /></div>
          <div className="kind-body">
            <div className="kind-title">
              <strong>{kind.name}</strong>
              <span className="arrow">→</span>
            </div>
            <span className="kind-like">{kind.like}</span>
            <dl className="kind-facts">
              <div><dt>Made of</dt><dd>{kind.madeOf}</dd></div>
              <div><dt>Read as</dt><dd>{kind.readAs}</dd></div>
            </dl>
            <code className="kind-field">{kind.field ?? `kind: ${kind.kind}`}</code>
          </div>
        </a>
      ))}
    </div>
  );
}
