import { expect, test } from "bun:test";
import { Registry } from "./registry.ts";
import { registerCore } from "./core-parts.ts";

test("core kinds describe entries and only updates and problems wake Fast", () => {
  const r = new Registry(":memory:");
  registerCore(r, {});
  const events: [string, boolean][] = [];
  r.onEntry = (entry, wakes) => { events.push([r.line(entry), wakes]); };
  const page = { by: "me", text: "Opened a page" };
  r.commit("page", { address: "/home" }, [{ kind: "page", fields: page }]);
  r.commit("session", {}, [{ kind: "problem", fields: { text: "Page unavailable" } }]);
  expect(events).toEqual([["page  Opened a page", false], ["problem  Page unavailable", true]]);
  expect(r.state("page").address).toBe("/home");
});
