// The live session's orb, in the bottom right corner of every page. Pressing it starts and stops the session. It
// shows the user's voice, the AI's voice, and whether work is running.
import { useEffect, useRef } from "react";
import { loud, start, stop, useLive } from "./live";
import { drawOrb } from "./live-orb";

export const css = `
  /* The orb sits in the corner, not in the middle where the place puts its widgets, and moves aside for the panel. */
  #devpage-place-float:has(.live-orb) { left: auto; right: 0; bottom: 0; transform: none; }
  body.with-panel #devpage-place-float:has(.live-orb) { right: 300px; }
  .live-orb { position: relative; width: 128px; height: 128px; pointer-events: none; }
  .live-orb canvas { display: block; width: 100%; height: 100%; }
  .live-orb button { position: absolute; left: 50%; top: 50%; width: 48px; height: 48px; margin: -24px 0 0 -24px; border: 0; padding: 0; border-radius: 50%; background: none; cursor: pointer; pointer-events: auto; }
  .live-orb button:focus-visible { outline: 2px solid var(--accent); outline-offset: 6px; }
  /* The marks on the page, each in the colour of its author's voice in the orb: the user's selection in red, the
     assistant's selection and the part it points at in its ice blue. */
  :root { --live-me: #e64c3c; --live-ai: #2f8fe6; }
  :root[data-theme="dark"] { --live-ai: #5cbdff; }
  @media (prefers-color-scheme: dark) { :root:not([data-theme]) { --live-ai: #5cbdff; } }
  mark.devpage-mark[data-mark="subject-user"] { background: color-mix(in srgb, var(--live-me) 26%, transparent); }
  mark.devpage-mark:is([data-mark="subject-assistant"], [data-mark="pointed"]) { background: color-mix(in srgb, var(--live-ai) 30%, transparent); }
  tspan.devpage-mark[data-mark="subject-user"] { fill: var(--live-me); }
  tspan.devpage-mark:is([data-mark="subject-assistant"], [data-mark="pointed"]) { fill: var(--live-ai); }
  .live-orb-problem { position: absolute; right: 104px; bottom: 44px; width: max-content; max-width: 320px; padding: 8px 12px; background: var(--background); border: 1px solid var(--border); border-radius: 8px; font-size: 13px; line-height: 1.4; color: var(--text); pointer-events: auto; }
`;

export default function DevpageLiveOrb() {
  const live = useLive();
  const canvas = useRef<HTMLCanvasElement>(null);
  const shown = useRef(live);
  shown.current = live;
  // While a voice that was on waits for the service to come back, the orb stays, as starting.
  const visible = live.connected || live.voice !== "off";

  useEffect(() => {
    if (!canvas.current) return;
    return drawOrb(canvas.current, () => ({
      on: shown.current.voice === "on",
      starting: shown.current.voice === "starting",
      me: loud.me,
      ai: loud.ai(),
      working: shown.current.tasks.some((task) => task.state === "running"),
    }));
  }, [visible]);

  if (!visible) return null;
  const on = live.voice !== "off";
  // The browser holds the sound back until a click: said here, since nothing can be heard.
  const note = live.quiet ? "Click anywhere to hear me" : live.problem;
  return (
    <div className="live-orb">
      <canvas ref={canvas} />
      {note && <span className="live-orb-problem" role="alert">{note}</span>}
      <button onClick={on ? stop : start} aria-pressed={on} aria-label={on ? "Stop the live session" : "Start the live session"} />
    </div>
  );
}
