// The live session at work: the page with the tasks panel open beside it.
// The test extension adds results below the running task.
import { Window, framesCss } from "./frames";
import { Orb, talkCss } from "./TalkPoint";

export const css = framesCss + talkCss + `
  .tt .w-body { padding: 0; display: flex; }
  .tt-page { flex: 1; padding: 34px 40px; display: flex; flex-direction: column; }
  .tt-foot { margin-top: auto; display: flex; justify-content: flex-end; padding: 20px 14px 14px 0; }
  .tt-panel { width: 600px; border-left: 1px solid var(--border); background: var(--surface); padding: 30px 34px; }
  .tt-heard { font-size: 26px; line-height: 1.4; color: var(--muted); margin: 0 0 26px; }
  .tt-task { display: flex; align-items: center; gap: 20px; padding: 22px 0; border-top: 1px solid var(--border); font-size: 30px; font-weight: 500; }
  .tt-task i { flex: none; width: 34px; height: 34px; border-radius: 50%; box-sizing: border-box; font-style: normal; }
  .tt-task i.runs { border: 5px solid var(--user); border-right-color: transparent; }
  .tt-task i.done { background: #34c759; color: white; display: grid; place-items: center; font-size: 22px; font-weight: 700; }
`;

export default function TalkTasks() {
  return (
    <div className="tt tk">
      <Window title="localhost:3030/docs/design/sync.md" style={{ width: "1560px" }}>
        <div className="tt-page">
          <h3>Retries</h3>
          <p>The app keeps every write in an outbox.</p>
          <p>It retries a failed write for two days.</p>
          <p>It stops when you sign out.</p>
          <div className="tt-foot"><Orb /></div>
        </div>
        <div className="tt-panel">
          <p className="tt-heard">Run the outbox tests.</p>
          <div className="tt-task"><i className="runs" />Run the outbox tests</div>
          <h3>Tests</h3>
          <div className="tt-task"><i className="done">✓</i>Keeps a failed write</div>
          <div className="tt-task"><i className="done">✓</i>Stops on sign-out</div>
        </div>
      </Window>
    </div>
  );
}
