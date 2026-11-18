// Codex, as a harness: each agent is a Codex thread on the user's own Codex login, run with `codex exec`. Everything that
// knows Codex is in this file: how a thread is continued or forked, how the live session's tools are handed to it,
// how its events become items, and where it keeps a thread so that it can be read back.
//
// A turn is one run of `codex exec --json`: it prints one event a line and ends when the turn does. The tools are
// served to it for the length of the turn by a small MCP server on this machine, which is how Codex takes tools.
import { randomUUID } from "node:crypto";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join, relative, sep } from "node:path";
import { z } from "zod";
import type { Harness, HarnessTool, Item, Model, PastTurn, Turn, TurnEnd, TurnEvent, TurnInput } from "./harness.ts";
import { shape } from "./log.ts";

/** The name the live session's tools are served under. */
const SERVER = "live";

function cut(text: unknown, most = 140) {
  const line = String(text ?? "").split("\n")[0].trim();
  return line.length > most ? `${line.slice(0, most)}…` : line;
}

/** A command as it was meant: Codex runs each one through a login shell, as a list or as one quoted string. */
function command(raw: unknown) {
  if (Array.isArray(raw)) return String(raw.length === 3 && /sh$/.test(String(raw[0])) ? raw[2] : raw.join(" "));
  const wrapped = /^\S*sh -l?c (['"])([\s\S]*)\1$/.exec(String(raw ?? ""));
  return wrapped ? wrapped[2] : String(raw ?? "");
}

/** The files of a change, which Codex gives as a list while a turn runs and by path when it is read back. */
function files(changes: unknown): string[] {
  if (Array.isArray(changes)) return changes.map((change) => String(change?.path ?? ""));
  return changes && typeof changes === "object" ? Object.keys(changes) : [];
}

/** The words of a message, which Codex gives as plain text while a turn runs and as parts when it is read back. */
function words(raw: any) {
  if (typeof raw.text === "string") return raw.text;
  return Array.isArray(raw.content) ? raw.content.map((part: any) => part.text ?? "").join("") : "";
}

/** Reads Codex's items as the live session's: the same way while a turn runs and when a thread is read back. */
class Reader {
  private readonly reads = new Map<string, NonNullable<HarnessTool["reads"]>>();

  /** `prefix` goes before every id: while a turn runs Codex numbers its items from zero again. */
  constructor(private readonly root: string, tools: HarnessTool[], private readonly prefix = "") {
    for (const given of tools) if (given.reads) this.reads.set(given.name, given.reads);
  }

  /** One item of Codex's as one of the live session's; nothing for what is not shown: its reasoning, the user's own message. */
  item(raw: any, at: number): Item | null {
    const id = `${this.prefix}${raw.id}`;
    // `command_execution` while a turn runs, `CommandExecution` in the file: one name for both.
    const type = String(raw.type ?? "").replace(/_/g, "").toLowerCase();
    const state: Item["state"] = raw.status === "in_progress" || raw.status === "inProgress" ? "running" : raw.status === "failed" || raw.status === "declined" || raw.error || (typeof raw.exit_code === "number" && raw.exit_code !== 0) ? "failed" : "done";
    const path = (value: unknown) => (typeof value === "string" && value.startsWith(this.root + sep) ? relative(this.root, value) : String(value ?? ""));
    switch (type) {
      case "agentmessage": {
        const text = words(raw).trim();
        return text ? { id, kind: "note", state: "done", at, line: text } : null;
      }
      case "commandexecution":
        return { id, kind: "command", state, at, line: `Ran ${cut(command(raw.command))}`, tool: "shell", detail: shape({ command: command(raw.command), exit_code: raw.exit_code }) };
      case "filechange":
        return { id, kind: "change", state, at, line: `Changed ${files(raw.changes).map(path).join(", ")}`, tool: "apply_patch", detail: shape(raw.changes) };
      case "mcptoolcall": {
        const reads = raw.server === SERVER ? this.reads.get(raw.tool) : undefined;
        if (!reads) return { id, kind: "tool", state, at, line: `${raw.server}: ${raw.tool}`, tool: `${raw.server}.${raw.tool}`, detail: shape(raw.arguments) };
        // One of the live session's own tools: what the agent said, not a tool it used.
        const said = reads(raw.arguments ?? {});
        return said ? { id, state: "done", at, ...said } : null;
      }
      case "websearch":
        return { id, kind: "search", state, at, line: `Searched the web for ${cut(raw.query)}`, tool: "web_search" };
      case "todolist":
        return { id, kind: "tool", state, at, line: "Updated its plan", tool: "todo_list", detail: shape(raw.items) };
      case "collabtoolcall":
        return { id, kind: "subagent", state, at, line: cut(raw.prompt ?? raw.tool ?? "Gave work to another agent"), tool: String(raw.tool ?? "collab") };
      case "error":
        return { id, kind: "notice", state: "failed", at, line: cut(raw.message, 400) };
      default:
        return null;
    }
  }
}

/** Serves the tools to one turn of Codex, as an MCP server over HTTP on this machine. Only who knows its address can call it. */
function serve(tools: HarnessTool[]) {
  const secret = randomUUID();
  const answer = async (call: any): Promise<unknown> => {
    switch (call.method) {
      case "initialize":
        return { protocolVersion: call.params?.protocolVersion ?? "2025-03-26", capabilities: { tools: {} }, serverInfo: { name: SERVER, version: "1" } };
      case "ping":
        return {};
      case "tools/list":
        return { tools: tools.map((given) => ({ name: given.name, description: given.description, inputSchema: z.toJSONSchema(z.object(given.input)) })) };
      case "tools/call": {
        const given = tools.find((one) => one.name === call.params?.name);
        if (!given) return { content: [{ type: "text", text: `No tool is called ${call.params?.name}.` }], isError: true };
        const input = z.object(given.input).safeParse(call.params?.arguments ?? {});
        if (!input.success) return { content: [{ type: "text", text: input.error.message }], isError: true };
        const { text, error } = await given.run(input.data);
        return { content: [{ type: "text", text }], ...(error ? { isError: true } : {}) };
      }
      default:
        throw new Error(`Method not found: ${call.method}`);
    }
  };
  const one = async (call: any) => {
    // A notification has no id and gets no answer.
    if (call.id === undefined || call.id === null) return null;
    try {
      return { jsonrpc: "2.0", id: call.id, result: await answer(call) };
    } catch (error) {
      return { jsonrpc: "2.0", id: call.id, error: { code: -32601, message: error instanceof Error ? error.message : String(error) } };
    }
  };
  const server = Bun.serve({
    hostname: "127.0.0.1",
    port: 0,
    async fetch(request) {
      if (new URL(request.url).pathname !== `/${secret}`) return new Response("not found", { status: 404 });
      if (request.method !== "POST") return new Response("only POST", { status: 405 });
      const body = await request.json();
      const answers = (await Promise.all((Array.isArray(body) ? body : [body]).map(one))).filter(Boolean);
      if (answers.length === 0) return new Response(null, { status: 202 });
      return Response.json(Array.isArray(body) ? answers : answers[0]);
    },
  });
  return { url: `http://127.0.0.1:${server.port}/${secret}`, close: () => void server.stop(true) };
}

/** The file Codex keeps of a thread: its name ends with the thread's id. */
function rollout(session: string) {
  const sessions = join(process.env.CODEX_HOME ?? join(homedir(), ".codex"), "sessions");
  if (!existsSync(sessions)) return null;
  for (const name of readdirSync(sessions, { recursive: true }) as string[]) if (name.endsWith(`${session}.jsonl`)) return join(sessions, name);
  return null;
}

export class Codex implements Harness {
  readonly name = "Codex";
  // Steer: Codex can queue a message for a thread, but nothing here uses it yet.
  readonly can = { fork: true, history: true, steer: false };

  /** `binary` is the `codex` to run: the one on the path by default. */
  constructor(private readonly binary = "codex") {}

  /** Codex's own catalog of models, without those it hides from a person. */
  async models(): Promise<Model[]> {
    const asked = Bun.spawn([this.binary, "debug", "models"], { stdout: "pipe", stderr: "ignore" });
    const catalog = JSON.parse(await new Response(asked.stdout).text());
    const all: any[] = Array.isArray(catalog) ? catalog : (catalog.models ?? []);
    return all.filter((model) => model.visibility !== "hide" && model.slug).map((model) => ({ id: String(model.slug), about: `${model.display_name ?? model.slug}: ${model.description ?? ""}`.trim() }));
  }

  turn(input: TurnInput): Turn {
    const began = Date.now();
    const tools = serve(input.tools);
    const reader = new Reader(input.root, input.tools, `${began}-`);
    // The options first, then what to go on with: nothing (a new thread), a thread, or a copy of one. The message is
    // given on the input, so that its length and its quotes do not matter.
    const args = [
      "exec",
      "--json",
      "--skip-git-repo-check",
      // It may change files in the project and in the folders given with it, and nowhere else. Nobody is there to
      // approve anything while the user talks.
      "--sandbox",
      "workspace-write",
      ...(input.also?.length ? ["-c", `sandbox_workspace_write.writable_roots=${JSON.stringify(input.also)}`] : []),
      "--cd",
      input.root,
      ...(input.model ? ["--model", input.model] : []),
      "-c",
      `developer_instructions=${JSON.stringify(input.brief)}`,
      "-c",
      `mcp_servers.${SERVER}.url=${JSON.stringify(tools.url)}`,
      // The live session's own tools need nobody's approval: they are how the agent reports.
      "-c",
      `mcp_servers.${SERVER}.default_tools_approval_mode="approve"`,
      ...(input.session ? [input.fork ? "fork" : "resume", input.session] : []),
      "-",
    ];
    const running = Bun.spawn([this.binary, ...args], { cwd: input.root, stdin: "pipe", stdout: "pipe", stderr: "pipe" });
    running.stdin.write(input.message);
    running.stdin.end();
    let stopped = false;

    async function* events(): AsyncGenerator<TurnEvent> {
      let end: TurnEvent | null = null;
      let problem = "";
      let last = "";
      /** The calls that have not ended yet. */
      const open = new Map<string, Item>();
      const errors = new Response(running.stderr).text();
      try {
        let rest = "";
        const decoder = new TextDecoder();
        for await (const chunk of running.stdout) {
          rest += decoder.decode(chunk, { stream: true });
          const lines = rest.split("\n");
          rest = lines.pop() ?? "";
          for (const text of lines) {
            let event: any;
            try {
              event = JSON.parse(text);
            } catch {
              // Not an event: Codex says a thing or two in plain words around them.
              continue;
            }
            if (event.type === "thread.started") {
              yield { type: "started", session: event.thread_id };
            } else if (event.type === "item.started" || event.type === "item.updated" || event.type === "item.completed") {
              const item = reader.item(event.item, Date.now());
              if (!item) continue;
              if (item.kind === "note") last = item.line;
              if (item.state === "running") open.set(item.id, item);
              else open.delete(item.id);
              yield { type: "item", item };
            } else if (event.type === "error") {
              problem = String(event.message ?? "");
            } else if (event.type === "turn.failed") {
              end = { type: "ended", state: stopped ? "stopped" : "failed", text: String(event.error?.message ?? problem ?? "The turn failed."), detail: { ms: Date.now() - began } };
            } else if (event.type === "turn.completed") {
              end = { type: "ended", state: stopped ? "stopped" : "done", text: last, detail: { ms: Date.now() - began, usage: shape(event.usage) } };
            }
          }
        }
        const code = await running.exited;
        // It ended without saying how the turn ended: it was stopped, or it could not start or go on with the thread.
        if (!end) {
          const said = (await errors).trim().split("\n").slice(-3).join(" ");
          if (!stopped && input.session && /not found|no (such )?(session|thread|rollout)|could not (resume|fork)/i.test(said)) throw new Error(`Codex could not go on with thread ${input.session}: ${said}`);
          end = { type: "ended", state: stopped ? "stopped" : "failed", text: problem || said || `Codex ended with code ${code}.`, detail: { ms: Date.now() - began, code } };
        }
        // A call that never ended, ends with the turn.
        const state: TurnEnd = end.type === "ended" ? end.state : "failed";
        for (const item of open.values()) yield { type: "item", item: { ...item, state: state === "done" ? "done" : "failed" } };
        yield end;
      } finally {
        tools.close();
        if (running.exitCode === null) running.kill();
      }
    }

    return {
      events: events(),
      stop: async () => {
        stopped = true;
        running.kill("SIGINT");
      },
    };
  }

  /** Read from the file Codex itself keeps of the thread. A turn starts at each message of the user's. */
  async history(session: string, input: Pick<TurnInput, "root" | "tools">): Promise<PastTurn[]> {
    const file = rollout(session);
    if (!file) return [];
    const reader = new Reader(input.root, input.tools);
    const turns: PastTurn[] = [];
    for (const text of readFileSync(file, "utf8").split("\n")) {
      if (!text.includes('"item_completed"')) continue;
      let line: any;
      try {
        line = JSON.parse(text);
      } catch {
        continue;
      }
      const raw = line.payload?.type === "item_completed" ? line.payload.item : null;
      if (!raw) continue;
      const when = Date.parse(line.timestamp ?? "") || 0;
      if (raw.type === "UserMessage") {
        turns.push({ started: when, items: [{ id: String(raw.id), kind: "user_message", state: "done", at: when, line: words(raw) }] });
        continue;
      }
      const item = reader.item(raw, when);
      const turn = turns.at(-1);
      if (!item || !turn) continue;
      const known = turn.items.findIndex((one) => one.id === item.id);
      if (known < 0) turn.items.push(item);
      else turn.items[known] = item;
    }
    return turns;
  }
}
