// The real service connects to a deterministic Voice, Fast and slow harness.
// The mock responses expose only the receipts the browser test checks.
import { mock } from "bun:test";
import { join } from "node:path";
import { writeFileSync, readFileSync } from "node:fs";
const root = process.env.DEVPAGE_ROOT;
const local = name => join(import.meta.dir, name);
const receipt = name => writeFileSync(join(root, name), "ok");
let dispatched = false;
const provider = Bun.serve({
  hostname: "127.0.0.1", port: 0,
  async fetch(request) {
    const body = await request.json();
    if (body.tool_choice === "none") return Response.json({ output: [] });
    const png = readFileSync(join(root, "fixture.png")).toString("base64");
    if (body.input.some(m => Array.isArray(m.content) && m.content.some(c => c.image_url === "data:image/png;base64," + png))) receipt("fast-saw-image");
    let code = "// Done.";
    if (!dispatched) {
      dispatched = true;
      code = 'mac.run("version"); mac.screenshot(); spawn(`The user said: "${m1}"`);';
    }
    return Response.json({ output: [{ type: "function_call", name: "exec", arguments: JSON.stringify({ code }), call_id: crypto.randomUUID() }] });
  },
});
mock.module(local("provider.ts"), () => ({ NEEDS: "", provider: () => ({ responses: provider.url.href, headers: {} }) }));
mock.module(local("voice.ts"), () => ({ Voice: class {
  started = Promise.resolve();
  constructor(_, events) {
    setTimeout(() => events.said({ who: "me", text: "Run the tests.", start_ms: 0, end_ms: 50, handover: "h1" }), 100);
  }
  stop() {}
}}));
mock.module(local("claude.ts"), () => ({ ClaudeCode: class {
  can = { fork: false, steer: false, history: false };
  models() { return Promise.resolve([]); }
  turn({ tools }) {
    const call = (name, args = {}) => tools.find(t => t.name === name).run(args);
    async function* events() {
      yield { type: "started", session: "test-agent" };
      const version = await call("mac_run", { script: "version" });
      if (!version.error && version.text.length) receipt("slow-ran-script");
      const shot = await call("mac_screenshot");
      if (shot.image?.data === readFileSync(join(root, "fixture.png")).toString("base64")) receipt("slow-saw-image");
      const run = await call("tests_run");
      if (run.error) throw new Error(run.text);
      yield { type: "ended", state: "done", text: "Running the tests." };
    }
    return { events: events(), stop: async () => {} };
  }
}}));
