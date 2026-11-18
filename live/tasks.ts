/** A part of a page, as devpage names it: a source's address, words in it, and the text around them. */
export interface Part {
  page: string;
  quote: string;
  prefix?: string;
  suffix?: string;
}

export interface Task {
  id: number;
  /** A few words, shown in the sidebar. */
  title: string;
  /** What was asked, in full. */
  request: string;
  state: "running" | "done" | "failed";
  /** The address the user was on when giving it. */
  page: string | null;
  /** The part the user had marked: what the task is about. It is not a comment on it. */
  part: Part | null;
  /** The agent that has it. Tasks from before the agents have none. */
  agent?: number;
  latest?: { id: number; type: import("./world.ts").Update["type"] };
  origin?: { session: string | null; handover: string };
  /** The newest update the agent published on it: its report, once it ended. */
  summary: string | null;
  /** The parts it changed. */
  changes: Part[];
  started: number;
  ended: number | null;
}

export class Tasks {
  constructor(private readonly list: Task[], private readonly changed: () => void) {
    // Work that was running when the service stopped did not report; it is shown as failed, not as running forever.
    for (const task of this.list) {
      if (task.state === "running") Object.assign(task, { state: "failed", summary: "The session restarted before it reported.", ended: Date.now() });
    }
  }

  all() {
    return this.list;
  }

  running() {
    return this.list.filter((task) => task.state === "running");
  }

  get(id: number) {
    return this.list.find((task) => task.id === id);
  }

  add(task: Pick<Task, "title" | "request" | "page" | "part" | "agent" | "origin">) {
    const id = Math.max(0, ...this.list.map((task) => task.id)) + 1;
    const added: Task = { ...task, id, state: "running", summary: null, changes: [], started: Date.now(), ended: null };
    this.list.push(added);
    this.changed();
    return added;
  }

  /** Ends a running task. Returns it, or nothing when it is unknown or already ended. */
  end(id: number, state: "done" | "failed", summary: string, changes: Part[]) {
    const task = this.get(id);
    if (!task || task.state !== "running") return undefined;
    Object.assign(task, { state, summary, changes, ended: Date.now() });
    this.changed();
    return task;
  }

  /** Changes a running task while it runs: its title, its agent, or its newest update. */
  change(id: number, fields: Partial<Pick<Task, "title" | "summary" | "agent" | "latest">>) {
    const task = this.get(id);
    if (!task || task.state !== "running") return undefined;
    Object.assign(task, fields);
    this.changed();
    return task;
  }

}
