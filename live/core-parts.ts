// The core's own parts are registered as a package's part is: one declared object each. The transcript and the
// sandbox use typed views of their entries.
import { coreParts } from "./events.ts";
import { line, type Entry } from "./world.ts";
import { z } from "zod";
import type { Kind, Now, PartFunction, Registry } from "./registry.ts";

export type CorePart = (typeof coreParts)[keyof typeof coreParts];

/** The entry kinds of a core part, each with the line Fast reads of one and whether one wakes Fast. */
export function coreEntries(part: CorePart): Record<string, Kind> {
  const kinds = Object.entries(coreParts).filter(([, owner]) => owner === part);
  return Object.fromEntries(kinds.map(([kind]) => [kind, { line: (fields: any) => line({ ...fields, kind } as Entry), wakes: kind === "update" || kind === "problem" }]));
}

/** Fast's calls the sandbox itself provides, as the registry checks one: they are written in Fast's prompt, and carried out by the service. */
const CALLS = {
  session: {
    say: { syntax: "say(text)", description: "Give Voice the line to speak.", input: { text: z.string() } },
    restart: { syntax: "restart(force)", description: "Restart the live session.", input: { force: z.boolean() } },
  },
  work: {
    spawn: { syntax: "spawn(prompt, route)", description: "Give work to a new agent.", input: { prompt: z.string(), route: z.string().optional() } },
    send: { syntax: "aN.send(prompt)", description: "Give work to an existing agent.", input: { agent: z.number(), prompt: z.string() } },
    fork: { syntax: "aN.fork(prompt)", description: "Give work to a copy of an agent.", input: { agent: z.number(), prompt: z.string() } },
  },
} satisfies Record<CorePart, Record<string, Omit<PartFunction, "for" | "run">>>;

export type CoreCall = keyof typeof CALLS.session | keyof typeof CALLS.work;

/** Registers the core's two parts. `carry` carries out one of Fast's own calls; without it they are checked and cannot be carried out, which is all an eval needs. `more` are the other functions of a core part. */
export function registerCore(registry: Registry, carry?: (call: CoreCall, input: any, now: Now) => unknown, more: Partial<Record<CorePart, Record<string, PartFunction>>> = {}) {
  for (const part of new Set(Object.values(coreParts))) {
    const calls = Object.entries(CALLS[part]).map(([name, call]): [string, PartFunction] => [name, {
      ...call,
      for: "fast",
      run: (input, now) => {
        if (!carry) throw new Error(`nothing carries out ${name} here`);
        return carry(name as CoreCall, input, now);
      },
    }]);
    void registry.part(part, { entries: coreEntries(part), functions: { ...more[part], ...Object.fromEntries(calls) } });
  }
}
