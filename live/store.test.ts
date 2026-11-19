import { expect, test } from "bun:test";
import { mkdtempSync, readFileSync, writeFileSync, existsSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { registerCore } from "./core-parts.ts";
import { Registry } from "./registry.ts";
import { SessionStore } from "./store.ts";
import { World } from "./world.ts";

test("an interrupted import keeps its sources and retries alongside existing extension entries", async () => {
  const dir = mkdtempSync(join(tmpdir(), "session-import-"));
  try {
    const registry = new Registry(join(dir, "world.sqlite"));
    registry.part("counter", p => {
      p.kind("count", { line: () => "count" });
      p.set("count", { for: "page", input: {}, description: "Count" }, (_, ctx) => ctx.add("count", {}));
    });
    await registry.call("counter.count", {}, { who: "page" });
    const file = join(dir, "world.jsonl");
    const message = { kind: "message" as const, who: "me" as const, text: "hello", page: null, marked: "", start_ms: null, end_ms: null, handover: null, n: 1, id: 10, at: "2026-01-01T00:00:00Z" };
    const source = JSON.stringify(message);
    writeFileSync(file, source + "\n{");
    expect(() => new SessionStore(dir)).toThrow();
    expect(existsSync(file)).toBe(true);
    writeFileSync(file, source);
    new SessionStore(dir).close();
    const store = new SessionStore(dir);
    expect(store.world()).toEqual([message]);
    expect(readFileSync(file + ".migrated", "utf8")).toBe(source);
    expect(registry.since(0).map(e => [e.n, e.kind, e.originalNumber])).toEqual([[1, "message", undefined], [2, "count", 1]]);
    registerCore(registry, {});
    const world = new World(registry);
    expect(world.next("message")).toBe(11);
    expect(world.add({ kind: "problem", text: "test" }).n).toBe(3);
    store.close();
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("a previously migrated SQLite session joins the shared stream once, retaining agent conversations", () => {
  const dir = mkdtempSync(join(tmpdir(), "session-upgrade-"));
  try {
    const first = new SessionStore(dir);
    const agent = { id: 3, session: "existing harness conversation" };
    first.set("agents", { agents: [agent] });
    const message = { n: 91, kind: "update", at: "2026-10-01", id: 14, task: 7, agent: 3, text: "done", type: "finished" };
    first.db.run("create table session_world (n integer primary key, value text not null)");
    first.db.run("insert into session_world values (?, ?)", [91, JSON.stringify(message)]);
    first.close();

    const upgraded = new SessionStore(dir);
    const registry = new Registry(upgraded.db);
    expect(registry.since(90)).toMatchObject([{ n: 91, part: "work", fields: { id: 14, task: 7 } }]);
    upgraded.close();
    const reopened = new SessionStore(dir);
    expect(reopened.world()).toEqual([message as import("./world.ts").Update]);
    expect(reopened.get("agents")).toEqual({ agents: [agent] });
    reopened.close();
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("one service owns a project until it exits; reports can read it throughout", async () => {
  const dir = mkdtempSync(join(tmpdir(), "session-owner-"));
  const owner = Bun.spawn([process.execPath, "-e", `
    import { SessionStore } from ${JSON.stringify(import.meta.resolve("./store.ts"))};
    const store = new SessionStore(process.argv[1]);
    store.set("owner", "first");
    console.log("ready");
    setInterval(() => store.get("owner"), 1000);`, dir], { stdout: "pipe", stderr: "pipe" });
  try {
    const { value } = await owner.stdout.getReader().read();
    expect(new TextDecoder().decode(value)).toContain("ready");
    expect(() => new SessionStore(dir)).toThrow("already serving");
    const reader = new SessionStore(dir, "read");
    expect(reader.get("owner")).toBe("first");
    reader.close();
    owner.kill("SIGKILL");
    await owner.exited;
    const next = new SessionStore(dir);
    expect(next.get("owner")).toBe("first");
    next.close();
  } finally {
    owner.kill();
    await owner.exited;
    rmSync(dir, { recursive: true, force: true });
  }
});
