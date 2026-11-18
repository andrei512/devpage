import { expect, test } from "bun:test";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
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
  const model = Bun.serve({
    hostname: "127.0.0.1", port: 0,
    async fetch(request) {
      const body = await request.json();
      if (body.tool_choice === "none") return Response.json({ output: [] });
      requests.push(body);
      const code = requests.length === 1 ? 'counter.change(2); spawn("Count three more")' : "// done";
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
      if (message.type === "action") ws.send(JSON.stringify({ type: "result", id: message.id, value: null }));
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
      db.run("pragma busy_timeout = 5000");
      const [update] = db.query("select fields from entries where part='work' and kind='update' order by n desc limit 1").values();
      const report = JSON.parse(JSON.parse(String(update[0])).text);
      expect(report.result).toEqual({ text: "counted 3" });
      expect(report.tools).toEqual(["counter_change", "read_log", "update", "write_comment"]);
    } finally { db.close(); }
  } finally {
    socket?.close();
    service.kill();
    await service.exited;
    await model.stop(true);
    rmSync(root, { recursive: true, force: true });
  }
}, 15000);
