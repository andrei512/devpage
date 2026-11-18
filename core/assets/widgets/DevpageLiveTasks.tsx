// The side panel: the agents, each with the tasks it has. An agent that works, or has a question for the user, is open, and stays open for five minutes after it is done: its
// tasks are under it, newest first, three at most and none that ended more than five minutes ago, with "Show more"
// for the rest. One that has been idle for longer is a single row with its last task, and a click opens it. A
// closed agent is not shown. Clicking a task goes to what it changed. Above the list, the last thing the session
// heard or said, in a box of a fixed height. A task is its title only. The agents are in two groups, each under a
// small muted heading, with a thin line between them: the ones that work or ask, and the ones from earlier.
//
// The same panel is shown in two places: flat on the right of a page, and as a card on the scratch pad's canvas.
// What is inside is the same in both.
//
// A click on an agent's name opens its chat in place of the list, and the panel gets twice as wide: the agent's
// session as one conversation, what the user said, its work folded to one line, what it answered, and a box to type to it.
// The arrow on top goes back to the list. The small arrow at the end of an agent's row folds and unfolds its tasks.
//
// A service from before it sent the agents gets the list it had: every task, newest first.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { AgentOrb } from "./live-agent-orb";
import { goTo, openAgent, tell, useLive, type Agent, type Item, type Task, type Turn } from "./live";

export const css = `
  /* As high as devpage's navbar, 52px with its line, so the line under it meets the navbar's. That is two lines of
     text. The height is fixed, so the list under it never moves. Short text starts at the top; of longer text the end
     shows, since the newest words are last. */
  /* It reaches over the panel's own 16px of padding on both sides, so the line under it goes from edge to edge. */
  /* It also reaches over the panel's 16px on top: text that scrolls up goes to the very top of the panel, fading out
     over its first lines, with no empty band above a hard edge. */
  .live-heard { display: flex; flex-direction: column-reverse; height: 52px; overflow-y: auto; scrollbar-width: none; margin: -16px -16px 14px; padding: 8px 16px 6px; -webkit-mask-image: linear-gradient(to bottom, transparent, rgba(0, 0, 0, 0.8) 10px, #000 22px); mask-image: linear-gradient(to bottom, transparent, rgba(0, 0, 0, 0.8) 10px, #000 22px); border-bottom: 1px solid var(--border); color: var(--muted); font-size: 13px; line-height: 1.45; }
  .live-heard::-webkit-scrollbar { display: none; }
  .live-heard span { margin-bottom: auto; }
  .live-heard.me { color: var(--text); }
  .live-tasks { list-style: none; margin: 0; padding: 0; }
  .live-tasks li { margin: 0; }
  .live-tasks .row { display: flex; align-items: flex-start; gap: 10px; padding: 8px; border-radius: 8px; color: var(--text); cursor: pointer; }
  .live-tasks .row:hover { background: var(--surface); }
  .live-tasks button { padding: 0; border: 0; background: none; color: inherit; font: inherit; text-align: left; cursor: pointer; }
  .live-tasks .title { display: block; width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .live-tasks svg { flex: none; margin-top: 2px; }
  .live-tasks .running svg:first-child { color: #e64c3c; }
  .live-tasks .running .row > svg { animation: live-turn 1s linear infinite; }
  .live-tasks .done svg:first-child { color: var(--accent); }
  .live-tasks .failed svg:first-child { color: #e64c3c; }
  .live-tasks .done span, .live-tasks .failed span { color: var(--muted); }
  .live-tasks span { flex: 1; min-width: 0; }
  .live-agents { list-style: none; margin: 0; padding: 0; }
  .live-agents > li { margin: 0 0 6px; }
  .live-agents .agent-row { display: flex; align-items: center; gap: 10px; width: 100%; padding: 8px 8px 8px 0; border: 0; border-radius: 8px; background: none; color: var(--text); font: inherit; text-align: left; cursor: pointer; }
  .live-agents .agent-row:hover { background: var(--surface); }
  .live-agents .agent-row .orb { flex: none; width: 22px; height: 22px; }
  .live-agents .agent-row .name { flex: 1; min-width: 0; }
  .live-agents .agent-row b { display: block; font-weight: 700; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .live-agents .agent-row small { display: block; margin-top: 1px; color: var(--muted); font-size: 12px; line-height: 1.4; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .live-agents .idle .agent-row b { font-weight: 500; color: var(--muted); }
  .live-agents .idle .agent-row .orb { opacity: 0.55; }
  .live-agents .asked .agent-row .orb { animation: live-flare 1.2s ease-in-out infinite; }
  .live-agents .live-tasks { padding-left: 6px; }
  .live-agents .live-tasks .row { padding: 3px 8px; }
  .live-agents .older { margin: 0 0 4px 14px; padding: 2px 0; border: 0; background: none; color: var(--muted); font: inherit; font-size: 12px; cursor: pointer; }
  .live-agents .older:hover { text-decoration: underline; }
  .live-group { margin: 0 0 4px; padding: 0; color: var(--muted); font-size: 12px; font-weight: 500; line-height: 1.4; }
  .live-agents + .live-group { margin-top: 10px; padding-top: 12px; border-top: 1px solid var(--border); }
  .live-agents .agent-head { display: flex; align-items: center; }
  .live-agents .agent-head .agent-row { flex: 1; min-width: 0; }
  .live-agents .fold { flex: none; display: inline-flex; align-items: center; justify-content: center; width: 26px; height: 26px; padding: 0; border: 0; border-radius: 6px; background: none; color: var(--muted); cursor: pointer; }
  .live-agents .fold:hover { background: var(--surface); color: var(--text); }
  .live-agents .fold svg { transition: transform 0.15s; }
  .live-agents .fold[aria-expanded="true"] svg { transform: rotate(90deg); }

  /* An agent's chat takes the panel's place, twice as wide; the page, the comments and the orb move aside for it. */
  body:has(.live-chat) #devpage-place-panel { width: min(600px, 100vw); padding: 0; overflow: hidden; }
  body.panel-beside:has(.live-chat) { padding-right: min(600px, 100vw); }
  body.panel-beside:has(.live-chat) #devpage-corner, body.panel-beside:has(.live-chat) #devpage-comments-toggle { right: 616px; }
  body.panel-beside:has(.live-chat) #devpage-comments:not(.beside) { right: 600px; }
  body.with-panel:has(.live-chat) #devpage-place-float:has(.live-orb) { right: min(600px, 100vw); }
  .live-chat { display: flex; flex-direction: column; height: 100vh; }
  /* As high as devpage's navbar, 52px with its line, so the two lines meet. */
  .live-chat header { flex: none; display: flex; align-items: center; gap: 10px; height: 52px; padding: 0 16px 0 10px; border-bottom: 1px solid var(--border); }
  .live-chat header button { flex: none; display: inline-flex; align-items: center; justify-content: center; width: 30px; height: 30px; padding: 0; border: 0; border-radius: 8px; background: none; color: var(--accent); cursor: pointer; }
  .live-chat header button:hover { background: var(--surface); }
  .live-chat header .orb { flex: none; width: 30px; height: 30px; }
  .live-chat header b { flex: 1; min-width: 0; font-size: 16px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .live-chat header small { flex: none; display: inline-flex; align-items: center; gap: 6px; color: var(--muted); font-size: 12px; }
  .live-chat header small svg { color: #e64c3c; animation: live-turn 1s linear infinite; }
  .live-chat .talk { flex: 1; overflow-y: auto; padding: 16px 20px; display: flex; flex-direction: column; gap: 10px; }
  .live-chat .empty { margin: auto; color: var(--muted); font-size: 13px; }
  .live-chat .mine { align-self: flex-end; max-width: 78%; margin-top: 10px; padding: 9px 13px; border-radius: 14px; background: color-mix(in srgb, var(--accent) 14%, transparent); color: var(--text); white-space: pre-wrap; overflow-wrap: anywhere; }
  .live-chat .its { margin: 0; color: var(--text); line-height: 1.5; white-space: pre-wrap; overflow-wrap: anywhere; }
  .live-chat .its.failed { color: #e64c3c; }
  .live-chat .its.question { padding-left: 10px; border-left: 3px solid var(--accent); }
  .live-chat .work > button { padding: 0; border: 0; background: none; color: var(--accent); font: inherit; font-size: 13px; cursor: pointer; text-align: left; }
  .live-chat .work > button:hover { text-decoration: underline; }
  .live-chat .work > button::before { content: "›"; display: inline-block; width: 14px; transition: transform 0.15s; }
  .live-chat .work > button[aria-expanded="true"]::before { transform: rotate(90deg); }
  .live-chat .steps { list-style: none; margin: 6px 0 0; padding: 8px 12px; border: 1px solid var(--border); border-radius: 8px; background: var(--background); font-size: 12px; line-height: 1.7; color: var(--muted); }
  .live-chat .steps li { margin: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .live-chat .steps .note { font-style: italic; }
  .live-chat .steps .failed { text-decoration: line-through; }
  .live-chat .steps .running { color: #e64c3c; font-weight: 600; }
  .live-chat form { flex: none; display: flex; align-items: flex-end; gap: 8px; margin: 0 16px 16px; padding: 8px 8px 8px 16px; border: 1px solid var(--border); border-radius: 12px; background: var(--background); }
  .live-chat form:focus-within { border-color: var(--accent); }
  .live-chat textarea { flex: 1; min-width: 0; max-height: 160px; padding: 7px 0; border: 0; outline: 0; resize: none; background: none; color: var(--text); font: inherit; line-height: 1.45; }
  .live-chat form button { flex: none; display: inline-flex; align-items: center; justify-content: center; width: 34px; height: 34px; padding: 0; border: 0; border-radius: 17px; background: var(--accent); color: var(--background); cursor: pointer; }
  .live-chat form button:disabled { opacity: 0.35; cursor: default; }
  @keyframes live-flare { 50% { transform: scale(1.3); filter: brightness(1.35) saturate(1.3); } }
  @keyframes live-turn { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) {
    .live-tasks .running .row > svg { animation-duration: 4s; }
    .live-agents .asked .agent-row .orb { animation: none; transform: scale(1.2); }
  }
`;

const MARKS = {
  running: "M21 12a9 9 0 1 1-6.2-8.6",
  done: "M4 12.5l5 5L20 6.5",
  failed: "M6 6l12 12M18 6L6 18",
};

function Icon({ d }: { d: string }) {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d={d} />
    </svg>
  );
}

function Row({ task }: { task: Task }) {
  return (
    <li className={task.state}>
      <div className="row" onClick={() => goTo(task)}>
        <Icon d={MARKS[task.state]} />
        <span>
          <button className="title" title={task.title} aria-label={`${task.title}: ${task.state}`}>
            {task.title}
          </button>
        </span>
      </div>
    </li>
  );
}

/** When an agent last had something happen: its newest task's start or end. */
function touched(tasks: Task[]) {
  return Math.max(0, ...tasks.map((task) => task.ended ?? task.started ?? task.id));
}

/** How many tasks an agent shows, and how long a task that ended stays among them. */
const SHOWN = 3;
const RECENT_MS = 5 * 60 * 1000;

function Group({ agent, tasks, chats }: { agent: Agent; tasks: Task[]; chats: boolean }) {
  // What the user's click chose: open or closed. Until the user clicks, the agent decides by itself.
  const [chosen, setChosen] = useState<boolean | null>(null);
  const [all, setAll] = useState(false);
  // Drawn again every half minute, so a task leaves the list once it has been over for five minutes.
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const tick = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(tick);
  }, []);
  // Newest first. Without "Show more": the tasks that run or ended in the last five minutes, three at most; when
  // there is none, the newest one.
  const newest = [...tasks].reverse();
  const recent = newest.filter((task) => task.state === "running" || now - (task.ended ?? now) < RECENT_MS).slice(0, SHOWN);
  const few = recent.length > 0 ? recent : newest.slice(0, 1);
  const shown = all ? newest : few;
  const idle = agent.state === "idle";
  // Open while it works or asks, and for five minutes after its last task ended: only then it folds to one row.
  const open = chosen ?? (!idle || now - touched(tasks) < RECENT_MS);
  const last = tasks[tasks.length - 1];
  // Until it has named itself, it goes by its first task.
  const name = agent.name ?? tasks[0].title;
  return (
    <li className={agent.state}>
      <div className="agent-head">
        {/* The name opens the agent's chat. A service that cannot show one: the row folds, as it did. */}
        <button className="agent-row" aria-label={chats ? `Open ${name}` : undefined} onClick={() => (chats ? openAgent(agent.id) : setChosen(!open))}>
          <AgentOrb id={agent.id} still={idle} className="orb" />
          <span className="name">
            <b>{name}</b>
            {!open && (
              <small>
                {last.state === "failed" ? "✕" : "✓"} {last.title}
              </small>
            )}
          </span>
        </button>
        {chats && (
          <button className="fold" aria-expanded={open} aria-label={open ? "Hide its tasks" : "Show its tasks"} onClick={() => setChosen(!open)}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M9 5l7 7-7 7" />
            </svg>
          </button>
        )}
      </div>
      {open && (
        <ul className="live-tasks">
          {shown.map((task) => (
            <Row key={task.id} task={task} />
          ))}
        </ul>
      )}
      {open && newest.length > few.length && (
        <button className="older" aria-expanded={all} onClick={() => setAll(!all)}>
          {all ? "Show less" : "Show more"}
        </button>
      )}
    </li>
  );
}

/** The kinds of item that are the agent's work, not what the user or the agent said. */
const WORK = new Set<Item["kind"]>(["note", "read", "change", "command", "search", "tool", "subagent", "notice"]);

/** "9 steps, 3 files changed": what a folded piece of work says. */
function summary(items: Item[]) {
  const steps = items.filter((item) => item.kind !== "note").length || items.length;
  const files = new Set(items.filter((item) => item.kind === "change" && item.state !== "failed").map((item) => item.line)).size;
  return `${steps} ${steps === 1 ? "step" : "steps"}, ${files === 0 ? "nothing changed" : `${files} ${files === 1 ? "file" : "files"} changed`}`;
}

/** A run of the agent's work between two messages. Folded to one line once it is over; open while it goes on. */
function Work({ items, going }: { items: Item[]; going: boolean }) {
  const [chosen, setChosen] = useState<boolean | null>(null);
  const open = chosen ?? going;
  return (
    <div className="work">
      <button aria-expanded={open} onClick={() => setChosen(!open)}>
        {going ? "Working" : summary(items)}
      </button>
      {open && (
        <ul className="steps">
          {items.map((item) => (
            <li key={item.id} className={`${item.kind} ${item.state}`} title={item.line}>
              {item.line}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** A session as one conversation: what the user said, the work in between as one piece, what the agent said. */
function pieces(turns: Turn[]) {
  const out: ({ kind: "mine" | "its"; item: Item } | { kind: "work"; key: string; items: Item[]; going: boolean })[] = [];
  for (const turn of turns) {
    let work: Item[] = [];
    const close = (going: boolean) => {
      if (work.length > 0) out.push({ kind: "work", key: work[0].id, items: work, going });
      work = [];
    };
    // A subagent's own work is inside its one line, and is not shown yet.
    for (const item of turn.items.filter((one) => !one.inside)) {
      if (WORK.has(item.kind)) {
        work.push(item);
        continue;
      }
      close(false);
      out.push({ kind: item.kind === "user_message" ? "mine" : "its", item });
    }
    // What comes after the last message of a turn that still runs is what the agent is doing now.
    close(turn.state === "running");
  }
  return out;
}

/** An agent's chat, in the panel's place. */
function Chat({ agent, tasks }: { agent: Agent | null; tasks: Task[] }) {
  const live = useLive();
  const [text, setText] = useState("");
  const talk = useRef<HTMLDivElement>(null);
  const session = live.session && live.session.agent === live.open ? live.session : null;
  const shown = session ? pieces(session.turns) : [];
  const count = session ? session.turns.reduce((sum, turn) => sum + turn.items.length, 0) : 0;
  // The newest is at the bottom, and stays in view as more arrives.
  useLayoutEffect(() => {
    if (talk.current) talk.current.scrollTop = talk.current.scrollHeight;
  }, [count, session === null]);
  const name = agent ? (agent.name ?? tasks[0]?.title ?? "An agent") : "An agent that was closed";
  const send = () => {
    if (!agent || !text.trim()) return;
    tell(agent.id, text);
    setText("");
  };
  return (
    <div className="live-chat">
      <header>
        <button aria-label="Back to the agents" title="Back to the agents" onClick={() => openAgent(null)}>
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M15 5l-7 7 7 7" />
          </svg>
        </button>
        {live.open != null && <AgentOrb id={live.open} still={agent?.state === "idle"} className="orb" />}
        <b>{name}</b>
        {agent?.state === "working" && (
          <small>
            <Icon d={MARKS.running} /> Working
          </small>
        )}
      </header>
      <div className="talk" ref={talk}>
        {session && shown.length === 0 && <p className="empty">Nothing of its session was kept: it worked before sessions were.</p>}
        {shown.map((piece) =>
          piece.kind === "work" ? (
            <Work key={piece.key} items={piece.items} going={piece.going} />
          ) : piece.kind === "mine" ? (
            <div key={piece.item.id} className="mine">
              {piece.item.line}
            </div>
          ) : (
            <p key={piece.item.id} className={`its ${piece.item.kind === "question" ? "question" : ""} ${piece.item.state === "failed" ? "failed" : ""}`}>
              {piece.item.line}
            </p>
          ),
        )}
      </div>
      {agent && (
        <form
          onSubmit={(event) => {
            event.preventDefault();
            send();
          }}
        >
          <textarea
            rows={1}
            value={text}
            placeholder="Message this agent"
            aria-label="Message this agent"
            onChange={(event) => setText(event.target.value)}
            onKeyDown={(event) => {
              // Enter sends, as in any chat; Shift and Enter is a new line.
              if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
                event.preventDefault();
                send();
              }
            }}
          />
          <button type="submit" disabled={!text.trim()} aria-label="Send">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
              <path d="M12 19V5M5 12l7-7 7 7" />
            </svg>
          </button>
        </form>
      )}
    </div>
  );
}

export default function DevpageLiveTasks() {
  const live = useLive();
  if (!live.connected) return null;
  const heard = live.voice === "on" && (
    <p className={`live-heard ${live.heard?.who ?? ""}`}>
      <span>{live.heard?.text}</span>
    </p>
  );
  // An agent's chat is open: it takes the place of the list.
  if (live.agents && live.chats && live.open != null) {
    const id = live.open;
    return <Chat agent={live.agents.find((agent) => agent.id === id) ?? null} tasks={live.tasks.filter((task) => task.agent === id)} />;
  }
  if (!live.agents) {
    return (
      <>
        {heard}
        <ul className="live-tasks">
          {[...live.tasks].reverse().map((task) => (
            <Row key={task.id} task={task} />
          ))}
        </ul>
      </>
    );
  }
  // The agents that work or wait for the user's answer first, then the idle ones; in each, the one touched last on top.
  const groups = live.agents
    .map((agent) => ({ agent, tasks: live.tasks.filter((task) => task.agent === agent.id) }))
    .filter((group) => group.tasks.length > 0)
    .sort((a, b) => Number(a.agent.state === "idle") - Number(b.agent.state === "idle") || touched(b.tasks) - touched(a.tasks));
  const list = (heading: string, some: typeof groups) =>
    some.length > 0 && (
      <>
        <p className="live-group">{heading}</p>
        <ul className="live-agents">
          {some.map(({ agent, tasks }) => (
            <Group key={agent.id} agent={agent} tasks={tasks} chats={live.chats === true} />
          ))}
        </ul>
      </>
    );
  return (
    <>
      {heard}
      {list("Working", groups.filter((group) => group.agent.state !== "idle"))}
      {list("Earlier", groups.filter((group) => group.agent.state === "idle"))}
    </>
  );
}
