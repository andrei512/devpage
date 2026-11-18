import { expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { Database } from "bun:sqlite";

test("the live service exposes one registered function to Fast and the slow harness and delivers its image", async () => {
  const root = mkdtempSync(join(tmpdir(), "devpage-agent-calls-"));
  const image = "data:image/png;base64,iVBORw0KGgo=";
  writeFileSync(join(root, "counter.ts"), `export default ({ kind, action, z }) => {
    kind("ready", { line: () => "The counter is ready", wakes: true });
    kind("counted", { line: f => f.who + " counted " + f.n, image: f => f.who === "fast" ? ${JSON.stringify(image)} : null,
      wakes: (_, caller) => caller.who === "fast" });
    action("start", { for: "page", input: {}, description: "Start counting" }, (_, ctx) => ctx.add("ready", {}));
    action("change", { for: "both", input: { n: z.number() }, description: "Count this many" }, ({ n }, ctx) => {
      ctx.set("total", (ctx.get("total") ?? 0) + n);
      ctx.add("counted", { n, who: ctx.caller.who });
      return "counted " + n;
    });
  }`);
  const requests: any[] = [];
  let handed = false;
  const model = Bun.serve({
    hostname: "127.0.0.1", port: 0,
    async fetch(request) {
      const body = await request.json();
      if (body.tool_choice === "none") return Response.json({ output: [] });
      requests.push(body);
      const marked = !handed && JSON.stringify(body.input.at(-1)).includes("Review this mark");
      if (marked) handed = true;
      const code = marked ? 'spawn(`Review: ${m1}`)' : requests.length === 1 ? 'counter.change(2); spawn("Count three more")' : "// done";
      return Response.json({ output: [{ type: "function_call", name: "exec", arguments: JSON.stringify({ code }), call_id: "call" + requests.length }] });
    },
  });
  const reservation = Bun.serve({ hostname: "127.0.0.1", port: 0, fetch: () => new Response() });
  const base = reservation.url.origin;
  const port = String(reservation.port);
  await reservation.stop(true);
  const service = Bun.spawn([process.execPath, "--preload", join(import.meta.dir, "service.test.harness.ts"), join(import.meta.dir, "service.ts")], {
    env: { HOME: root, PATH: process.env.PATH, DEVPAGE_ROOT: root, DEVPAGE_URL: base, DEVPAGE_PORT: port, DEVPAGE_TOKEN: "test",
      DEVPAGE_CONFIG: JSON.stringify({ world: { counter: "./counter.ts" } }), TEST_RESPONSES: model.url.href },
    stderr: "pipe", stdout: "ignore",
  });
  let socket: WebSocket | undefined;
  const headers = { "x-devpage-token": "test" };
  try {
    for (let i = 0; i < 200; i++) {
      if (service.exitCode !== null) throw new Error(await new Response(service.stderr).text());
      if (await fetch(base + "/state", { headers }).then(r => r.ok, () => false)) break;
      await Bun.sleep(20);
    }
    const ServiceSocket = WebSocket as unknown as new (url: string, options: Bun.WebSocketOptions) => WebSocket;
    socket = new ServiceSocket(base.replace("http:", "ws:") + "/socket", { headers });
    let started = false;
    const ws = socket;
    ws.onopen = () => ws.send(JSON.stringify({ type: "start" }));
    ws.onmessage = event => {
      const message = JSON.parse(event.data);
      if (message.type === "action") ws.send(JSON.stringify({ type: "result", id: message.id, value: { address: "/home", text: "The marked line." } }));
      if (!started && (message.voice === "on" || message.part === "session" && message.state.voice === "on")) {
        started = true;
        ws.send(JSON.stringify({ type: "call", name: "counter.start", input: {} }));
      }
    };
    for (let i = 0; i < 150 && !requests.some(r => JSON.stringify(r.input).includes("counted 3")); i++) await Bun.sleep(25);
    expect(requests[0]?.instructions).toContain("counter.change(n)");
    expect(requests[0]?.instructions).not.toContain("counter.start(");
    const seen = requests.flatMap(r => r.input.filter((m: any) => m.role === "user").map((m: any) => m.content)).flat();
    expect(seen).toContainEqual({ type: "input_image", image_url: image });
    expect(JSON.stringify(seen)).toContain("slow counted 3");
    const db = new Database(join(root, ".devpage/live/projects", root.replace(/[^a-zA-Z0-9]/g, "-"), "world.sqlite"), { readonly: true });
    try {
      db.run("pragma busy_timeout=3000");
      const [update] = db.query("select fields from entries where part='work' and kind='update' order by n desc limit 1").values();
      const report = JSON.parse(JSON.parse(String(update[0])).text);
      expect(report.result).toEqual({ text: "counted 3" });
      expect(report.tools).toEqual(["counter_change", "read_log", "update", "write_comment"]);
      // A real hand-over through the sandbox carries the marked part to work.
      const part = { page: "/home", quote: "marked line", prefix: "The ", suffix: "." };
      ws.send(JSON.stringify({ type: "page", page: { address: "/home", fragment: "", sources: ["/home"] }, focused: true }));
      ws.send(JSON.stringify({ type: "subject", part }));
      ws.send(new Uint8Array([0, 0]));
      const work = () => JSON.parse(String(db.query("select value from state where part='work' and key='tasks'").values()[0][0]));
      for (let i = 0; i < 120 && work().length < 2; i++) await Bun.sleep(20);
      const second = work()[1];
      expect(second.part).toEqual(part);
      expect(second.origin.handover).toBe("h-mark");
      expect(typeof second.origin.session).toBe("string");
      const transcript = db.query("select fields from entries where kind='message' order by n desc limit 1").values();
      expect(JSON.parse(String(transcript[0][0])).part).toEqual(part);

      // Freeze in the isolated project after its module has gone. Replay must
      // see the same extension functions and image without loading a package.
      rmSync(join(root, "counter.ts"));
      const replay = Bun.spawnSync([process.execPath, "-e", `
        import { freeze, moment } from "./eval";
        import { run } from "./sandbox";
        const session = freeze("", process.env.HOME, process.env.HOME);
        const index = session.runs.findIndex(r => r.images?.length);
        const at = await moment({ name: "counter", why: "", session: "", run: index + 1, expect: { does: [] } }, session);
        const ran = await run("counter.change(4)", at.scope, at.registry);
        console.log(JSON.stringify({ intent: ran.intents[0], content: at.input.at(-1).content }));
      `], { cwd: import.meta.dir, env: { HOME: root, PATH: process.env.PATH } });
      expect(replay.exitCode, replay.stderr.toString()).toBe(0);
      const frozen = JSON.parse(replay.stdout.toString().trim().split("\n").at(-1)!);
      expect(frozen.intent).toEqual({ do: "world", name: "counter.change", input: { n: 4 } });
      expect(frozen.content).toContainEqual({ type: "input_image", image_url: image });
      expect(frozen.content[0].text).toContain("fast counted 2");
    } finally { db.close(); }
  } finally {
    socket?.close();
    service.kill();
    await service.exited;
    await model.stop(true);
    rmSync(root, { recursive: true, force: true });
  }
}, 15000);

test("the real service restores the work, page and canvas snapshots after a restart", async () => {
  const root = mkdtempSync(join(tmpdir(), "devpage-core-parts-"));
  const state = join(root, ".devpage/live/projects", root.replace(/[^a-zA-Z0-9]/g, "-"));
  mkdirSync(join(state, "pads"), { recursive: true });
  const task = { id: 12, title: "Earlier work", request: "Do it", state: "done", page: "/", part: null,
    summary: "Finished", changes: [], started: 1, ended: 2 };
  writeFileSync(join(state, "tasks.json"), JSON.stringify([task]));
  writeFileSync(join(state, "pads/idea.md"), "# Our idea\n");
  const mark = { page: "pads/idea.md", quote: "Our idea", prefix: "", suffix: "" };
  const headers = { "x-devpage-token": "test" };

  async function serve() {
    const reserve = Bun.serve({ hostname: "127.0.0.1", port: 0, fetch: () => new Response() });
    const base = reserve.url.origin;
    const port = String(reserve.port);
    await reserve.stop(true);
    const service = Bun.spawn([process.execPath, "--preload", join(import.meta.dir, "service.test.harness.ts"), join(import.meta.dir, "service.ts")], {
      env: { HOME: root, PATH: process.env.PATH, DEVPAGE_ROOT: root, DEVPAGE_URL: base, DEVPAGE_PORT: port, DEVPAGE_TOKEN: "test" },
      stderr: "pipe", stdout: "ignore",
    });
    try {
      for (let i = 0; i < 200; i++) {
        if (service.exitCode !== null) throw new Error(await new Response(service.stderr).text());
        if (await fetch(base + "/state", { headers }).then(r => r.ok, () => false)) break;
        await Bun.sleep(20);
      }
      const Socket = WebSocket as unknown as new (url: string, options: Bun.WebSocketOptions) => WebSocket;
      const socket = new Socket(base.replace("http:", "ws:") + "/socket", { headers });
      const events: { type: string; part: string; state: any }[] = [];
      socket.onmessage = message => { events.push(JSON.parse(message.data)); };
      const received = async (size: number) => {
        for (let i = 0; i < 200 && events.length < size; i++) await Bun.sleep(10);
        expect(events.length).toBeGreaterThanOrEqual(size);
        return events.splice(0, size);
      };
      return { received, send: (message: unknown) => socket.send(JSON.stringify(message)), close: async () => {
        socket.close();
        service.kill();
        await service.exited;
      } };
    } catch (error) {
      service.kill();
      await service.exited;
      throw error;
    }
  }

  try {
    const initial = await serve();
    try {
      const parts = Object.fromEntries((await initial.received(4)).map(e => [e.part, e.state]));
      expect(Object.keys(parts).sort()).toEqual(["canvas", "page", "session", "work"]);
      expect(parts.work.tasks).toEqual([task]);
      initial.send({ type: "canvas", open: true, address: "pads/idea.md" });
      expect((await initial.received(2)).map(e => e.part).sort()).toEqual(["canvas", "page"]);
      initial.send({ type: "subject", part: mark });
      expect(await initial.received(1)).toMatchObject([{ part: "page", state: { subject: mark } }]);
    } finally { await initial.close(); }

    const next = await serve();
    try {
      const parts = Object.fromEntries((await next.received(4)).map(e => [e.part, e.state]));
      expect(parts.page).toMatchObject({ address: "pads/idea.md", subject: mark, by: "user" });
      expect(parts.canvas.canvas.pad).toBe("pads/idea.md");
      expect(parts.work.tasks).toEqual([task]);
    } finally { await next.close(); }
  } finally { rmSync(root, { recursive: true, force: true }); }
}, 15000);
