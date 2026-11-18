import { expect, test } from "bun:test";
import { mkdtempSync, readFileSync, writeFileSync, existsSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
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
    new SessionStore(dir).db.close();
    const store = new SessionStore(dir);
    expect(store.world()).toEqual([message]);
    expect(readFileSync(file + ".migrated", "utf8")).toBe(source);
    expect(registry.since(0)).toHaveLength(1);
    const world = new World(store, () => {});
    expect(world.next("message")).toBe(11);
    expect(world.add({ kind: "problem", text: "test" }).n).toBe(2);
    store.db.close();
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
