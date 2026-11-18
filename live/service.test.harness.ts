// Loaded only in the test service subprocess. Keep dispatch, storage, sockets,
// the sandbox and context builder real. Replace the model and harness transports.
import type { VoiceEvents } from "./voice.ts";
import { mock } from "bun:test";
import type { TurnInput, TurnEvent } from "./harness.ts";

mock.module("./provider.ts", () => ({
  NEEDS: "test provider",
  provider: () => ({ responses: process.env.TEST_RESPONSES, headers: {} }),
}));
mock.module("./voice.ts", () => ({
  Voice: class {
    started = Promise.resolve();
    constructor(_provider: unknown, private events: VoiceEvents) {}
    hear() {
      this.events.said({ who: "me", text: "Review this mark", handover: "h-mark", start_ms: 10, end_ms: 20 });
    }
    stop() {}
  },
}));
mock.module("./claude.ts", () => ({
  ClaudeCode: class {
    can = { fork: false, steer: false, history: false };
    models() { return Promise.resolve([]); }
    turn({ tools, message }: TurnInput) {
      async function* events(): AsyncGenerator<TurnEvent> {
        yield { type: "started", session: "test-agent" };
        const tool = tools.find(t => t.name === "unit-tests_change");
        const result = tool && await tool.run({ n: 3 });
        const report = JSON.stringify({ result, tools: tools.map(t => t.name).sort() });
        const update = tools.find(t => t.name === "update")!;
        const answer = await update.run({ task: Number(/^Task (\d+)/.exec(message)?.[1]), type: "finished", text: report });
        if (answer.error) throw new Error(answer.text);
        yield { type: "ended", state: "done", text: "The registered update already ended this task" };
      }
      return { events: events(), stop: async () => {} };
    }
  },
}));
