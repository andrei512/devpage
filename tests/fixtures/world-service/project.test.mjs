import test from "node:test";
import { existsSync } from "node:fs";
import { setTimeout } from "node:timers/promises";

test("first result", () => {});
test("second result", async () => {
  for (let i = 0; i < 200 && !existsSync("continue-tests"); i++) await setTimeout(50);
  if (!existsSync("continue-tests")) throw new Error("the panel never saw the first result");
});
