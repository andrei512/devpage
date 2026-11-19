// A placeholder for a part of a scratch pad that is not drawn yet. An agent
// writes one for each part it plans, as its first save, and then puts the
// real part in the place of each. It has no text: its shape says what is
// coming, and its size is about the size of the real part, so the pad does
// not jump. Every placeholder breathes; one is alive, the next part to be
// drawn: a light runs around it and its inside draws itself.
//
// <Soon kind="flow" />                   a flow of steps; `steps` says how many
// <Soon kind="cards" count={2} />        cards side by side
// <Soon kind="list" />                   a list
// <Soon kind="table" columns={3} />
// <Soon kind="picture" />
// <Soon kind="text" />
//
// Size, on any kind:
//   height="s" | "m" | "l" | "xl", or a number of pixels
//   width="quarter" | "third" | "half" | "two-thirds" | "three-quarters"; full when left out
// Placeholders that follow each other and fit in one row sit side by side.
// What is inside follows the size: a taller list has more lines, a taller
// table more rows. `lines` and `rows` set it exactly.
//
// The alive one is the first placeholder on the page: an agent fills them in
// from the top. Nothing on a placeholder can be pressed or changed by the one
// who looks at it: the agent sets the kind and the size where it writes it.
import { useEffect, useRef, useState } from "react";

type Kind = "flow" | "cards" | "list" | "table" | "picture" | "text";

export const css = `
  @property --soon-turn { syntax: "<angle>"; inherits: false; initial-value: 0deg; }

  /* The look: a wireframe. Grey shapes inside a hairline frame; the alive one is red: its frame, and the red that is
     written over its shapes. One red, the site's, and no other colour. No gradient, no glow. */
  .soon { --soon-rest: color-mix(in srgb, var(--text) 9%, transparent); --soon-strong: color-mix(in srgb, var(--text) 17%, transparent);
    --soon-red: #e64c3c; --soon-ink: color-mix(in srgb, var(--soon-red) 78%, transparent); --soon-wash: color-mix(in srgb, var(--soon-red) 16%, transparent);
    position: relative; box-sizing: border-box; margin: 6px 12px 6px 0; padding: 22px 24px; border: 1px solid var(--border); border-radius: 10px;
    transition: border-color .5s, background .5s; }
  .soon.now { border-color: color-mix(in srgb, var(--soon-red) 55%, transparent); background: color-mix(in srgb, var(--soon-red) 3%, transparent); }

  /* One grid for all of them. A placeholder narrower than the page takes its exact share of the page's width, and the
     ones that follow each other share a row. Each leaves the gap on its right, a full one too, so a row of several
     ends on the same edge as a full one. What follows a row starts under it. */
  div[data-widget="Soon"]:has(> .soon-w) { float: left; }
  div[data-widget="Soon"]:has(> .soon-w-quarter) { width: 25%; }
  div[data-widget="Soon"]:has(> .soon-w-third) { width: calc(100% / 3); }
  div[data-widget="Soon"]:has(> .soon-w-half) { width: 50%; }
  div[data-widget="Soon"]:has(> .soon-w-two-thirds) { width: calc(200% / 3); }
  div[data-widget="Soon"]:has(> .soon-w-three-quarters) { width: 75%; }
  div[data-widget="Soon"]:has(> .soon-w) + :not(div[data-widget="Soon"]:has(> .soon-w)) { clear: both; }
  /* A cleared element loses its top margin, so text after a row gets its distance as padding. */
  div[data-widget="Soon"]:has(> .soon-w) + :not(div[data-widget="Soon"]) { padding-top: 1.3em; }
  article:has(.soon-w)::after { content: ""; display: block; clear: both; }
  .soon-w { padding: 18px 18px; }

  /* A short piece of the frame of the alive one is solid red, and travels around it. */
  .soon::before { content: ""; position: absolute; inset: -1px; border-radius: inherit; padding: 1.5px; pointer-events: none; opacity: 0; transition: opacity .5s;
    background: conic-gradient(from var(--soon-turn), transparent 0deg, var(--soon-red) 5deg, var(--soon-red) 34deg, transparent 39deg);
    -webkit-mask: linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0); -webkit-mask-composite: xor; mask-composite: exclude;
    animation: soon-turn 4.2s linear infinite; }
  .soon.now::before { opacity: 1; }
  @keyframes soon-turn { to { --soon-turn: 360deg; } }

  /* A shape: grey and breathing while it waits. When alive, ink is written over it from the left, behind a caret. */
  .soon-s { position: relative; overflow: hidden; border-radius: 4px; background: var(--soon-rest); animation: soon-breathe 3.6s ease-in-out infinite; animation-delay: calc(var(--i, 0) * .2s); }
  @keyframes soon-breathe { 0%, 100% { opacity: .6; } 50% { opacity: 1; } }
  .soon-s::after { content: ""; position: absolute; top: 0; bottom: 0; left: 0; width: 0; box-sizing: border-box; background: var(--soon-ink); border-right: 2px solid var(--soon-red); opacity: 0; }
  .soon.now .soon-s { animation: none; }
  .soon.now .soon-s::after { animation: soon-write 3.8s cubic-bezier(.4, 0, .2, 1) infinite; animation-delay: calc(var(--i, 0) * .28s); }
  @keyframes soon-write { 0% { width: 0; opacity: 1; border-right-color: var(--soon-red); } 38% { width: 100%; opacity: 1; border-right-color: var(--soon-red); } 44% { width: 100%; opacity: 1; border-right-color: transparent; } 84% { width: 100%; opacity: 1; border-right-color: transparent; } 100% { width: 100%; opacity: 0; border-right-color: transparent; } }

  .soon-line { height: 8px; }
  .soon-stack { display: flex; flex-direction: column; justify-content: center; gap: 16px; height: 100%; overflow: hidden; }

  .soon-flow { position: relative; display: flex; align-items: center; height: 100%; }
  .soon-node { flex: 1; height: 45%; min-height: 26px; max-height: 96px; border-radius: 6px; }
  .soon-link { flex: 0 0 7%; height: 1px; background: var(--soon-strong); }
  /* In a flow each step is washed in red, one after the other. */
  .soon.now .soon-node::after { width: 100%; border-right: 0; background: var(--soon-wash); animation: soon-step 3.8s ease-in-out infinite; animation-delay: calc(var(--i, 0) * (3.2s / var(--n, 4))); }
  @keyframes soon-step { 0% { opacity: 0; } 6% { opacity: 1; } 70% { opacity: 1; } 100% { opacity: 0; } }

  .soon-cards { display: flex; gap: 14px; height: 100%; }
  .soon-card { flex: 1; min-width: 0; overflow: hidden; display: flex; flex-direction: column; justify-content: center; gap: 14px; padding: 16px 18px; border: 1px solid var(--border); border-radius: 6px; }
  .soon-head { height: 12px; width: 42%; background: var(--soon-strong); }

  .soon-item { display: flex; align-items: center; gap: 12px; }
  .soon-bullet { flex: none; width: 6px; height: 6px; border-radius: 50%; background: var(--soon-strong); }
  .soon-bullet::after, .soon-cell::after { border-right: 0; }

  .soon-table { display: grid; gap: 8px 10px; height: 100%; align-content: center; }
  .soon-cell { height: 12px; border-radius: 3px; }
  .soon-cell.head { background: var(--soon-strong); }

  /* A picture is uncovered from the left, behind a red line, again and again. */
  .soon-picture { height: 100%; min-height: 60px; border-radius: 6px; display: flex; align-items: center; justify-content: center; color: color-mix(in srgb, var(--text) 30%, transparent); }
  .soon.now .soon-picture::after { background: var(--soon-wash); animation: soon-uncover 3.8s cubic-bezier(.4, 0, .2, 1) infinite; }
  @keyframes soon-uncover { 0% { width: 0; opacity: 1; } 70% { width: 100%; opacity: 1; } 84% { width: 100%; opacity: 1; border-right-color: transparent; } 100% { width: 100%; opacity: 0; border-right-color: transparent; } }

  @media (prefers-reduced-motion: reduce) { .soon::before, .soon-s, .soon-s::after { animation-duration: 12s !important; } }
`;

// Which placeholder is alive, for all of them on the page: the first one.
const all = new Map<HTMLElement, (alive: boolean) => void>();

function first() {
  const order = [...all.keys()].filter((el) => el.isConnected).sort((a, b) => (a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1));
  for (const [el, set] of all) set(el === order[0]);
}

const widths = ["92%", "68%", "81%", "56%", "74%", "88%"];
const range = (n: number) => Array.from({ length: Math.max(1, Math.round(n)) }, (_, i) => i);
const at = (i: number) => ({ "--i": i }) as React.CSSProperties;

function Lines({ lines, from = 0 }: { lines: number; from?: number }) {
  return <>{range(lines).map((i) => <div className="soon-s soon-line" style={{ ...at(from + i), width: widths[i % widths.length] }} />)}</>;
}

const heights = { s: 84, m: 150, l: 260, xl: 400 };
type Width = "quarter" | "third" | "half" | "two-thirds" | "three-quarters" | "full";

export default function Soon({ kind = "text", height = "m", width = "full", lines, steps, count = 2, rows, columns = 3 }: {
  kind?: Kind; height?: number | keyof typeof heights; width?: Width; lines?: number; steps?: number; count?: number; rows?: number; columns?: number;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [now, setNow] = useState(false);
  const set = typeof height === "number" ? height : heights[height] ?? heights.m;
  // The size it really has on the page: what is inside follows it.
  const [size, setSize] = useState({ wide: 700, tall: set });

  useEffect(() => {
    const el = ref.current!;
    all.set(el, setNow);
    first();
    const watch = new ResizeObserver(() => setSize({ wide: el.offsetWidth, tall: el.offsetHeight }));
    watch.observe(el);
    return () => { all.delete(el); watch.disconnect(); first(); };
  }, []);

  const room = size.tall - 40;
  const fit = (each: number, least = 1) => Math.max(least, Math.floor(room / each));
  lines ??= fit(26);
  rows ??= fit(25, 2) - 1;
  steps ??= Math.max(2, Math.min(5, Math.floor(size.wide / 170)));
  const inCard = Math.max(1, Math.min(6, Math.floor((room - 36 - 29) / 24)));
  const narrow = width !== "full";

  return (
    <div ref={ref} className={`soon ${now ? "now" : ""} ${narrow ? `soon-w soon-w-${width}` : ""}`} style={{ height: set }} aria-hidden="true">
      {kind === "flow" && (
        <div className="soon-flow" style={{ "--n": steps } as React.CSSProperties}>
          {range(steps).flatMap((i) => [i > 0 && <div className="soon-link" />, <div className="soon-s soon-node" style={at(i)} />])}
        </div>
      )}
      {kind === "cards" && (
        <div className="soon-cards">
          {range(count).map((c) => (
            <div className="soon-card">
              <div className="soon-s soon-head" style={at(c * 3)} />
              <Lines lines={inCard} from={c * 3 + 1} />
            </div>
          ))}
        </div>
      )}
      {kind === "list" && (
        <div className="soon-stack">
          {range(lines).map((i) => (
            <div className="soon-item">
              <div className="soon-s soon-bullet" style={at(i)} />
              <div className="soon-s soon-line" style={{ ...at(i), width: widths[i % widths.length] }} />
            </div>
          ))}
        </div>
      )}
      {kind === "table" && (
        <div className="soon-table" style={{ gridTemplateColumns: `repeat(${columns}, 1fr)` }}>
          {range((rows + 1) * columns).map((i) => <div className={`soon-s soon-cell ${i < columns ? "head" : ""}`} style={at(i * 0.6)} />)}
        </div>
      )}
      {kind === "picture" && (
        <div className="soon-s soon-picture">
          <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round"><rect x="3" y="4" width="18" height="16" rx="2" /><circle cx="9" cy="10" r="1.6" /><path d="M4 18l5-5 4 4 3-3 4 4" /></svg>
        </div>
      )}
      {kind === "text" && <div className="soon-stack"><Lines lines={lines} /></div>}
    </div>
  );
}
