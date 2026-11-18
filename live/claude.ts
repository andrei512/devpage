// Claude Code, as a harness: each agent is a Claude Code session on the user's own Claude login, run with the Claude Agent
// SDK. Everything that knows Claude Code is in this file: how a session is continued or forked, how the live session's
// tools are handed to it, and how its messages become items.
import { relative, resolve, sep } from "node:path";
import { createSdkMcpServer, getSessionMessages, query, tool, type Options, type SDKUserMessage } from "@anthropic-ai/claude-agent-sdk";
import type { Harness, HarnessTool, Item, ItemKind, Model, PastTurn, Turn, TurnEnd, TurnEvent, TurnInput } from "./harness.ts";
import { toolReply } from "./harness.ts";
import { shape } from "./log.ts";

/** The name the live session's tools are served under: Claude Code calls them `mcp__live__<name>`. */
const SERVER = "live";

/** Claude Code's own tools, as kinds of item. Any other tool is a `tool`. */
const KINDS: Record<string, ItemKind> = {
  Read: "read",
  WebFetch: "read",
  Edit: "change",
  MultiEdit: "change",
  Write: "change",
  NotebookEdit: "change",
  Bash: "command",
  Grep: "search",
  Glob: "search",
  WebSearch: "search",
  Agent: "subagent",
  Task: "subagent",
};

function cut(text: unknown, most = 140) {
  const line = String(text ?? "").split("\n")[0].trim();
  return line.length > most ? `${line.slice(0, most)}…` : line;
}

/** One line that says what a tool call is. */
function lineOf(name: string, input: any, root: string) {
  const path = (value: unknown) => (typeof value === "string" && value.startsWith(root + sep) ? relative(root, value) : String(value ?? ""));
  switch (name) {
    case "Read":
      return `Read ${path(input.file_path)}`;
    case "WebFetch":
      return `Read ${cut(input.url)}`;
    case "Edit":
    case "MultiEdit":
    case "Write":
      return `Changed ${path(input.file_path)}`;
    case "NotebookEdit":
      return `Changed ${path(input.notebook_path)}`;
    case "Bash":
      return `Ran ${cut(input.command)}`;
    case "Grep":
      return `Searched for ${cut(input.pattern)}`;
    case "Glob":
      return `Looked for files ${cut(input.pattern)}`;
    case "WebSearch":
      return `Searched the web for ${cut(input.query)}`;
    case "Agent":
    case "Task":
      return cut(input.description ?? input.prompt);
    default:
      // A tool of some other server: `mcp__server__name` reads better as its two names.
      return name.startsWith("mcp__") ? name.slice(5).replace("__", ": ") : name;
  }
}

/** Reads Claude Code's messages as items: the same way while a turn runs and when a conversation is read back. */
class Reader {
  /** The tool calls that have not answered yet, by Claude's id for the call. */
  readonly calls = new Map<string, Item>();
  private readonly reads = new Map<string, NonNullable<HarnessTool["reads"]>>();

  constructor(private readonly root: string, tools: HarnessTool[]) {
    for (const given of tools) if (given.reads) this.reads.set(`mcp__${SERVER}__${given.name}`, given.reads);
  }

  /** The items of one message: new ones from the agent, and those a tool's answer ended. */
  items(message: { type: string; uuid?: string; message?: any; parent_tool_use_id?: string | null }, at: number): Item[] {
    const out: Item[] = [];
    const content = message.message?.content;
    const blocks: any[] = Array.isArray(content) ? content : [];
    if (message.type === "assistant") {
      const inside = message.parent_tool_use_id ? { inside: message.parent_tool_use_id } : {};
      blocks.forEach((block, index) => {
        if (block.type === "text" && block.text.trim()) {
          out.push({ id: `${message.uuid}-${index}`, kind: "note", state: "done", at, line: block.text.trim(), ...inside });
        } else if (block.type === "tool_use") {
          const reads = this.reads.get(block.name);
          if (reads) {
            // One of the live session's own tools: what the agent said, not a tool it used.
            const said = reads(block.input ?? {});
            if (said) out.push({ id: block.id, state: "done", at, ...said, ...inside });
            return;
          }
          const item: Item = { id: block.id, kind: KINDS[block.name] ?? "tool", state: "running", at, line: lineOf(block.name, block.input ?? {}, this.root), tool: block.name, detail: shape(block.input), ...inside };
          this.calls.set(block.id, item);
          out.push(item);
        }
      });
    } else if (message.type === "user") {
      // A tool's answer comes back as a user message: it ends the item of the call.
      for (const block of blocks) {
        const item = block.type === "tool_result" ? this.calls.get(block.tool_use_id) : undefined;
        if (!item) continue;
        this.calls.delete(block.tool_use_id);
        out.push({ ...item, state: block.is_error ? "failed" : "done" });
      }
    }
    return out;
  }
}

export class ClaudeCode implements Harness {
  readonly name = "Claude Code";
  // Steer: a turn's input stays open, but nothing here sends a second message on it yet.
  readonly can = { fork: true, history: true, steer: false };


  turn(input: TurnInput): Turn {
    let ended = () => {};
    const open = new Promise<void>((done) => (ended = done));
    // The message, and then nothing: the input stays open until the turn has ended, because the tools answer on it.
    async function* messages(): AsyncGenerator<SDKUserMessage> {
      yield { type: "user", message: { role: "user", content: input.message }, parent_tool_use_id: null };
      await open;
    }
    const running = query({ prompt: messages(), options: this.options(input) });
    let stopped = false;
    const reader = new Reader(input.root, input.tools);

    async function* events(): AsyncGenerator<TurnEvent> {
      const began = Date.now();
      const calls = reader.calls;
      try {
        for await (const message of running) {
          if (message.type === "system" && message.subtype === "init") {
            yield { type: "started", session: message.session_id, model: message.model };
          } else if (message.type === "assistant" || message.type === "user") {
            for (const item of reader.items(message, Date.now())) yield { type: "item", item };
          } else if (message.type === "result") {
            const failed = message.subtype !== "success" || message.is_error;
            const state: TurnEnd = stopped ? "stopped" : failed ? "failed" : "done";
            // A call that never answered ends with the turn.
            for (const item of calls.values()) yield { type: "item", item: { ...item, state: state === "done" ? "done" : "failed" } };
            calls.clear();
            yield {
              type: "ended",
              state,
              text: message.subtype === "success" ? message.result : `The session ended with ${message.subtype}.`,
              detail: { result: message.subtype, ms: Date.now() - began, api_ms: message.duration_api_ms, turns: message.num_turns, cost_usd: message.total_cost_usd, usage: shape(message.usage) },
            };
            ended();
          }
        }
      } finally {
        ended();
      }
    }

    return {
      events: events(),
      stop: async () => {
        stopped = true;
        await running.interrupt();
      },
    };
  }

  /** Read from the file Claude Code itself keeps of the conversation. A turn starts at each message that is not a tool's answer. */
  async history(session: string, input: Pick<TurnInput, "root" | "tools">): Promise<PastTurn[]> {
    const reader = new Reader(input.root, input.tools);
    const turns: PastTurn[] = [];
    const at = new Map<string, PastTurn>();
    for (const message of await getSessionMessages(session, { dir: input.root })) {
      const when = Date.parse((message as { timestamp?: string }).timestamp ?? "") || 0;
      const content = (message.message as any)?.content;
      const said = message.type !== "user" || message.parent_tool_use_id ? null : typeof content === "string" ? content : Array.isArray(content) && !content.some((block) => block.type === "tool_result") ? content.map((block) => (block.type === "text" ? block.text : "")).join("") : null;
      if (said !== null) {
        turns.push({ started: when, items: [{ id: message.uuid, kind: "user_message", state: "done", at: when, line: said }] });
        continue;
      }
      const turn = turns.at(-1);
      if (!turn) continue;
      for (const item of reader.items(message, when)) {
        // A call's answer is the same item again, ended: it takes the place of the call, in the turn the call was in.
        const of = at.get(item.id) ?? turn;
        const known = of.items.findIndex((one) => one.id === item.id);
        if (known < 0) of.items.push(item);
        else of.items[known] = { ...item, at: of.items[known].at };
        at.set(item.id, of);
      }
    }
    return turns;
  }

  /** Asked of Claude Code itself, which starts for it and runs no turn. */
  async models(): Promise<Model[]> {
    let ended = () => {};
    const open = new Promise<void>((done) => (ended = done));
    async function* nothing(): AsyncGenerator<SDKUserMessage> {
      await open;
    }
    const asked = query({ prompt: nothing(), options: { env: this.env(), settingSources: [] } });
    try {
      return (await asked.supportedModels()).map((model) => ({ id: model.value, about: `${model.displayName}: ${model.description}` }));
    } finally {
      ended();
      await asked.return(undefined).catch(() => {});
    }
  }

  /** The session runs on the machine's Claude Code login, never on an API key some other tool left in the environment. */
  private env() {
    const { ANTHROPIC_API_KEY: _, ...env } = process.env;
    return env;
  }

  private options(input: TurnInput): Options {
    const { root } = input;
    const served = input.tools.map((given: HarnessTool) =>
      tool(
        given.name,
        given.description,
        given.input,
        async (args: any) => toolReply(await given.run(args)),
        given.always ? { alwaysLoad: true } : undefined,
      ),
    );
    return {
      cwd: root,
      ...(input.model ? { model: input.model } : {}),
      ...(input.session ? { resume: input.session, forkSession: input.fork } : {}),
      env: this.env(),
      settingSources: ["project"],
      systemPrompt: { type: "preset", preset: "claude_code", append: input.brief },
      mcpServers: { [SERVER]: createSdkMcpServer({ name: SERVER, tools: served }) },
      // Nobody is there to approve prompts while the user talks, so everything is allowed except changing files outside
      // the project and the folders given with it.
      permissionMode: "default",
      canUseTool: async (name, args) => {
        const path = args.file_path ?? args.notebook_path;
        const edits = ["Edit", "Write", "MultiEdit", "NotebookEdit"].includes(name);
        if (edits && typeof path === "string" && ![root, ...(input.also ?? [])].some((folder) => resolve(root, path).startsWith(folder + sep))) {
          return { behavior: "deny", message: "Only files inside the project folder may be changed." };
        }
        return { behavior: "allow", updatedInput: args };
      },
    };
  }
}
