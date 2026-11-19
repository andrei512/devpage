// The canvas: a scratch pad that fills the whole screen, with a thin gray edge around it, while the user and the assistant work out an idea that is not in the docs. The
// service says which pad is on it. The pad is a page the session's program answers, drawn by devpage in a frame, so its widgets and
// diagrams work as on any page; the frame is the same site, so this widget reaches into it: it hides devpage's own
// frame around the page there, and hands the pad's window to the session, which hears the user's clicks on it and marks it.
// When the pad's file changes, it is loaded again in a second frame behind the first, and the two are swapped once it
// is drawn, at the same scroll: the new one fades in over the old, so a part that took a placeholder's place appears
// softly and nothing flashes. The cross is in the canvas's top right corner. Under it, the side panel as a card: the
// same component as on a page, in a rounded container with a border, as high as what is in it.
import { useEffect, useRef, useState } from "react";
import { closeCanvas, framed, padAddress, padShown, scroller, useLive } from "./live";
import DevpageLiveTasks, { css as panel } from "./DevpageLiveTasks";

export const css = `
  /* Full screen: the edge is a plain gray that hides the page, and the canvas is on it, 6px in, with rounded corners
     and no shadow. The pad's text keeps devpage's own column, in the middle. The rounding is said with more weight
     than the brand's "no rounding", which would win on a branded page. */
  .live-canvas-edge { position: fixed; inset: 0; z-index: 2147483000; background: var(--border); pointer-events: auto; }
  .live-canvas { position: fixed; inset: 6px; z-index: 2147483001; background: var(--background); overflow: hidden; pointer-events: auto; }
  html body .live-canvas { border-radius: 12px !important; }
  .live-canvas iframe { position: absolute; inset: 0; width: 100%; height: 100%; border: 0; background: var(--background); }
  .live-canvas iframe.behind { visibility: hidden; }
  .live-canvas iframe.front { animation: live-canvas-in .45s ease; }
  @keyframes live-canvas-in { from { opacity: 0; } to { opacity: 1; } }
  .live-canvas .close { position: absolute; top: 8px; right: 8px; z-index: 1; width: 32px; height: 32px; padding: 0; border: 0; border-radius: 6px; background: var(--background); color: var(--muted); cursor: pointer; display: flex; align-items: center; justify-content: center; }
  .live-canvas .close:hover { background: var(--surface); color: var(--text); }
  /* The side panel, as a card under the cross. Its padding is the panel's own 16px, which what is inside counts on.
     It ends above the orb. An agent's chat makes it twice as wide and as high as it may be. */
  .live-canvas .side { position: absolute; top: 48px; right: 14px; z-index: 1; box-sizing: border-box; width: 300px; max-width: calc(100% - 28px); max-height: calc(100% - 48px - 84px); padding: 16px; overflow-x: hidden; overflow-y: auto; border: 1px solid var(--border); background: var(--background); color: var(--text); font-size: 14px; }
  html body .live-canvas .side { border-radius: 12px !important; }
  .live-canvas .side:empty { display: none; }
  .live-canvas .side:has(.live-chat) { width: 600px; height: calc(100% - 48px - 84px); padding: 0; overflow: hidden; }
  .live-canvas .side .live-chat { height: 100%; }
  /* The orb stays above the canvas, and moves to the canvas's bottom right corner: the side panel, which it stands
     beside on a page, is under the canvas now. */
  body:has(.live-canvas) .live-orb { position: relative; z-index: 2147483002; }
  body:has(.live-canvas) #devpage-place-float:has(.live-orb) { left: auto !important; right: 14px !important; bottom: 10px !important; transform: none !important; }
  @media (max-width: 860px) { body:has(.live-canvas) #devpage-place-float:has(.live-orb) { right: 0 !important; bottom: 0 !important; } }
` + panel;

/** What devpage draws around a page: not shown on the canvas. */
const HIDDEN = `
  #devpage-navbar, #devpage-nav, #devpage-navigator, #devpage-sidebar, #devpage-corner, #devpage-comments, #devpage-outline,
  #devpage-place-panel, #devpage-place-float, .comment-ui, .comment-button { display: none !important; }
  html, body { background: transparent; }
`;

/** A pad as it is drawn: its address in the session, such as `pads/how-it-works.md`, and the number of its file's last change. */
interface Shown {
  pad: string;
  version: number;
}
const key = (shown: Shown) => `${shown.pad}@${shown.version}`;

/** Waits until devpage has drawn the page in a frame, widgets and all. False when it never said so. */
async function drawn(frame: HTMLIFrameElement) {
  for (let tries = 0; tries < 80; tries += 1) {
    if (frame.contentDocument?.documentElement.dataset.ready === "true") return true;
    await new Promise((done) => setTimeout(done, 50));
  }
  return false;
}

function Pad({ shown, front, ready }: { shown: Shown; front: boolean; ready: (frame: HTMLIFrameElement) => void }) {
  return (
    <iframe
      data-live-canvas=""
      className={front ? "front" : "behind"}
      src={padAddress(shown.pad)}
      title="Scratch pad"
      onLoad={(event) => {
        const frame = event.currentTarget;
        const inner = frame.contentDocument;
        if (!inner) return;
        const style = inner.createElement("style");
        style.textContent = HIDDEN;
        inner.head.append(style);
        void drawn(frame).then(() => ready(frame));
      }}
    />
  );
}

export default function DevpageLiveCanvas() {
  const live = useLive();
  const wanted = (!framed && live.connected && live.canvas) || null;
  /** The pad in front, once it is drawn. */
  const [front, setFront] = useState<Shown | null>(null);
  /** The pad that was in front before, kept under the new one while that fades in. */
  const [leaving, setLeaving] = useState<Shown | null>(null);
  const frames = useRef(new Map<string, HTMLIFrameElement>());

  // Closed, or another idea: nothing of the pad before is kept.
  useEffect(() => {
    if (wanted && front && wanted.pad === front.pad) return;
    setFront(null);
    setLeaving(null);
    padShown(null);
  }, [wanted?.pad]);
  useEffect(() => () => padShown(null), []);

  if (!wanted) return null;
  const same = front && key(front) === key(wanted);

  const ready = (shown: Shown) => (frame: HTMLIFrameElement) => {
    frames.current.set(key(shown), frame);
    // The same pad, drawn again: it is put where the one before it was scrolled to.
    const before = front && frames.current.get(key(front));
    try {
      const top = before?.contentDocument ? scroller(before.contentDocument).scrollTop : 0;
      if (top && frame.contentDocument) scroller(frame.contentDocument).scrollTop = top;
    } catch {}
    if (front) frames.current.delete(key(front));
    setLeaving(front);
    setTimeout(() => setLeaving((still) => (still === front ? null : still)), 500);
    setFront(shown);
    padShown(frame.contentWindow);
  };

  return (
    <>
      <div className="live-canvas-edge" />
      <div className="live-canvas" role="dialog" aria-modal="true" aria-label="Scratch pad">
        {leaving && <Pad key={key(leaving)} shown={leaving} front ready={() => {}} />}
        {front && <Pad key={key(front)} shown={front} front ready={() => {}} />}
        {!same && <Pad key={key(wanted)} shown={wanted} front={false} ready={ready(wanted)} />}
        <button className="close" onClick={closeCanvas} aria-label="Close the scratch pad">
          <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round">
            <path d="M6 6l12 12M18 6L6 18" />
          </svg>
        </button>
        <div className="side">
          <DevpageLiveTasks />
        </div>
      </div>
    </>
  );
}
