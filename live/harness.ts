// The harness: what the live session needs of an agent harness, whichever it is. Claude Code, Codex and any other are
// each one implementation of `Harness`. It is one abstraction for both directions: giving an agent a message to work on,
// and reading what happens inside it, as a session of turns and items that has the same shape for every harness.
//
// Nothing here knows about tasks. A task is the live session's own: a piece of work the user asked for. A turn is the
// harness's: one message in, and the agent works until it stops. Nothing outside an implementation asks which harness
// it is talking to; it reads `can`.
import type { z } from "zod";

/** The kinds of thing that happen in a turn. The list is closed: a tool that fits none of them is a `tool`. */
export type ItemKind =
  | "user_message" // what the user said or typed
  | "agent_message" // what the agent says to the user
  | "note" // what the agent says to itself while it works
  | "read" // it read a file or a page
  | "change" // it made or changed a file
  | "command" // it ran something in the shell
  | "search" // it searched the project or the web
  | "tool" // any other tool, with its name
  | "question" // it cannot go on without the user
  | "subagent" // it gave part of the work to an agent of its own
  | "notice"; // about the session, not from the agent: an error, a session that was lost

/** One thing that happened in a turn. */
export interface Item {
  /** Its own within the session. The same item is given again, with the same id, when its state changes. */
  id: string;
  kind: ItemKind;
  state: "running" | "done" | "failed";
  /** When it began, in ms. */
  at: number;
  /** One line of plain text that says what it is: what a screen shows. */
  line: string;
  /** The subagent item it happened inside, when it is part of a subagent's work. */
  inside?: string;
  /** The harness's own name for the tool, for the kinds that are tools. */
  tool?: string;
  /** What is behind the line, cut to a size that can be kept: a tool's input. Not shown until asked for. */
  detail?: unknown;
}

/** What ends a turn. `stopped`: it was stopped from outside before it ended by itself. */
export type TurnEnd = "done" | "failed" | "stopped";

/** What a running turn gives, in order: once that it started, then its items, then once how it ended. */
export type TurnEvent =
  | { type: "started"; /** The harness's own name for the conversation: what a later turn continues. */ session: string; model?: string }
  | { type: "item"; item: Item }
  | { type: "ended"; state: TurnEnd; /** The agent's last words, or why it failed. */ text: string; /** The harness's own numbers on the turn: time, cost, usage. For the log. */ detail?: Record<string, unknown> };

/** A tool the live session gives its agents, whatever the harness: how an agent reports back. */
export interface HarnessTool {
  name: string;
  description: string;
  /** Its input, as a zod shape. */
  input: Record<string, z.ZodType>;
  run: (input: any) => Promise<ToolResult>;
  /** The agent always has it at hand, also where a harness loads tools only when asked. */
  always?: boolean;
  /** How a call of it reads in a session, in place of a tool item: an update is what the agent says to the user. Nothing: the call is left out. */
  reads?: (input: any) => Pick<Item, "kind" | "line"> & Partial<Pick<Item, "state">> | null;
}

/** A registered function can return text together with an inline image. */
export interface ToolResult {
  text: string;
  image?: { mimeType: string; data: string };
  error?: boolean;
}

/** Both agent harnesses carry the same text and image blocks over MCP. */
export function toolReply({ text, image, error }: ToolResult) {
  const content = [{ type: "text" as const, text }, ...(image ? [{ type: "image" as const, ...image }] : [])];
  return { content, ...(error ? { isError: true } : {}) };
}

/** What a turn starts with. */
export interface TurnInput {
  /** The conversation to go on with, as a `started` event named it. None: a new agent. */
  session: string | null;
  /** Leave `session` as it is and go on in a copy of it: the new agent starts with what that one knows. Only where `can.fork`. */
  fork: boolean;
  /** The message that starts the turn. */
  message: string;
  /** What every agent of the live session is told about its role, on top of the harness's own instructions. */
  brief: string;
  tools: HarnessTool[];
  /** The project folder: the agent works in it and changes nothing outside it, but for `also`. */
  root: string;
  /** Folders outside the project whose files the agent may change too: where the scratch pads are. */
  also?: string[];
  /** The model the turn runs on, as `models` names it. None: the harness's own default. */
  model?: string | null;
}

/** A model a harness has. */
export interface Model {
  /** What the harness calls it: what a turn's `model` takes. */
  id: string;
  /** A line on what it is for, in the harness's own words. */
  about: string;
}

/** A turn that is over, or still runs, as the harness kept it: read back, not watched. */
export interface PastTurn {
  /** When its message was sent, in ms; 0 when the harness does not say. */
  started: number;
  /** The first is the message that started it. An item whose call has no answer yet is still `running`. */
  items: Item[];
}

/** A turn that runs. */
export interface Turn {
  /** Read once, to its end. It throws when the conversation could not be continued or copied. */
  events: AsyncIterable<TurnEvent>;
  /** Stops it before it ends by itself: it then ends as `stopped`. */
  stop: () => Promise<void>;
}

/** What a harness can do beyond the least. What it cannot, the live session does another way. */
export interface Capabilities {
  /** A new agent can start as a copy of another one's conversation. Without it, a new agent starts empty. */
  fork: boolean;
  /** It keeps its conversations and can give their turns again: `history`. Without it, only a turn that runs can be watched. */
  history: boolean;
  /** It takes a message while a turn is running. Without it, the message waits for the turn to end. */
  steer: boolean;
}

export interface Harness {
  /** For people and the log. No code decides anything by it. */
  readonly name: string;
  readonly can: Capabilities;
  /** Starts a turn: on a new agent, on one that exists, or on a copy of one. Its items do not include the message that started it. */
  turn: (input: TurnInput) => Turn;
  /** The models it has now, for the account it runs on. */
  models: () => Promise<Model[]>;
  /** A conversation's turns, oldest first, read from what the harness itself keeps: nothing is kept twice. Where `can.history`.
      A conversation that began as a copy of another starts with that one's turns. `tools` says how their calls read. */
  history?: (session: string, input: Pick<TurnInput, "root" | "tools">) => Promise<PastTurn[]>;
}
