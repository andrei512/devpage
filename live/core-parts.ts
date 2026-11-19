// Core kinds use the same registration and event format as a package. The
// existing transcript and sandbox use typed views of these entries.
import { coreParts } from "./events.ts";
import { line, type Entry } from "./world.ts";
import type { Registering, Registry } from "./registry.ts";

export type CorePart = (typeof coreParts)[keyof typeof coreParts] | "canvas";

export function registerCore(registry: Registry, functions: Partial<Record<CorePart, (as: Registering) => void>>) {
  for (const part of ["work", "page", "canvas", "session"] as const) {
    registry.part(part, as => {
      for (const [kind, owner] of Object.entries(coreParts)) {
        if (owner !== part) continue;
        as.kind(kind, { line: fields => line({ ...fields, kind } as Entry), wakes: kind === "update" || kind === "problem" });
      }
      functions[part]?.(as);
    });
  }
}
