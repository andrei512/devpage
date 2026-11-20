import { expect, test } from "bun:test";
import { Registry } from "./registry.ts";
import { registerCore } from "./core-parts.ts";
import { run } from "./sandbox.ts";

test("the core's entries have their lines, and only updates and problems wake Fast", () => {
  const r = new Registry(":memory:");
  registerCore(r);
  const events: [string, boolean][] = [];
  r.onEntry = (entry, wakes) => { events.push([r.line(entry), wakes]); };
  r.commit("session", {}, [{ kind: "message", fields: { id: 1, who: "me", text: "Hello", page: null, marked: "", start_ms: null, end_ms: null, handover: null } }]);
  r.commit("session", {}, [{ kind: "problem", fields: { text: "Page unavailable" } }]);
  expect(events.map(([, wakes]) => wakes)).toEqual([false, true]);
  expect(events[0][0]).toContain('"Hello"');
  expect(events[1][0]).toBe("problem  Page unavailable");
});

test("the core's calls are checked in a script wherever the core is registered, and are carried out only where something carries them", async () => {
  const checkedOnly = new Registry(":memory:");
  registerCore(checkedOnly);
  const scope = { messages: [], updates: [], agents: [1] };
  expect((await run('say("One"); a1.send("More")', scope, checkedOnly)).intents).toEqual([{ do: "say", text: "One" }, { do: "send", agent: 1, prompt: "More" }]);
  await expect(checkedOnly.call("session.say", { text: "One" }, { who: "fast", run: 1, handover: null, used: [] })).rejects.toThrow("nothing carries out say here");

  const carried: [string, unknown][] = [];
  const served = new Registry(":memory:");
  registerCore(served, (call, input) => void carried.push([call, input]));
  await served.call("work.spawn", { prompt: "Do it" }, { who: "fast", run: 1, handover: null, used: [] });
  expect(carried).toEqual([["spawn", { prompt: "Do it" }]]);
});
