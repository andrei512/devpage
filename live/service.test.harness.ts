// Loaded only in the test service subprocess. Keep dispatch, storage, sockets,
// the sandbox and context builder real. Replace the model and harness transports.
import { mock } from "bun:test";
import type { TurnInput, TurnEvent } from "./harness.ts";

mock.module("./provider.ts", () => ({
  NEEDS: "test provider",
  provider: () => ({ responses: process.env.TEST_RESPONSES, headers: {} }),
}));
mock.module("./voice.ts", () => ({
  Voice: class {
    started = Promise.resolve();
    stop() {}
  },
}));
mock.module("./claude.ts", () => ({
  ClaudeCode: class {
    can = { fork: false, steer: false, history: false };
    models() { return Promise.resolve([]); }
    turn({ tools }: TurnInput) {
      async function* events(): AsyncGenerator<TurnEvent> {
        yield { type: "started", session: "test-agent" };
        const tool = tools.find(t => t.name === "counter_change");
        const result = tool && await tool.run({ n: 3 });
        yield { type: "ended", state: "done", text: JSON.stringify({ result, tools: tools.map(t => t.name).sort() }) };
      }
      return { events: events(), stop: async () => {} };
    }
  },
}));
