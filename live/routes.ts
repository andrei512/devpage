// The routes: which harness and which model an agent may run on. A route is one harness and one of its models, named
// `harness/model`. The settings say which are enabled; the harnesses say which models exist; the rules in `routing.md`
// say which kind of work goes where, in plain words that only Fast reads. Both are set once for every project, in
// devpage's folder in the home directory, and a project that has its own replaces them whole.
//
// Nothing here decides where a piece of work goes. Fast decides, in the script it writes, from what `told` puts into
// its instructions. This file only says what Fast may choose from, and whether a name it chose is one of them.
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import type { Harness } from "./harness.ts";

/** A harness and one of its models. */
export interface Route {
  /** `harness/model`, or the harness alone when it runs on its own default model. */
  name: string;
  /** The harness, as the settings name it: `claude-code`, `codex`. */
  harness: string;
  /** None: the harness's own default model. */
  model: string | null;
  /** What the harness says the model is for. Empty until the harness was asked, and for its default model. */
  about: string;
}

/** The harnesses that are on, each with the models of it that may be used, in the order they were written. */
export type Enabled = [harness: string, models: string[]][];

const named = (harness: string, model: string | null) => (model ? `${harness}/${model}` : harness);

/** The settings: what `harnesses` holds, in the project's settings or else in the ones for every project. Nothing when there are none. */
export function settings(harnesses: unknown, problem: (text: string) => void): Enabled | null {
  if (harnesses == null) return null;
  if (typeof harnesses !== "object" || Array.isArray(harnesses)) {
    problem("The live session's `harnesses` setting is not a list of harnesses with their models.");
    return null;
  }
  const enabled: Enabled = Object.entries(harnesses).map(([harness, models]) => [harness, Array.isArray(models) ? models.map(String) : []]);
  return enabled.length > 0 ? enabled : null;
}

/** The routes the settings enable, before any harness was asked what it has: the first is the default. A harness nobody knows is left out. */
export function enabledRoutes(enabled: Enabled, known: (harness: string) => boolean, problem: (text: string) => void): Route[] {
  const routes: Route[] = [];
  for (const [harness, models] of enabled) {
    if (!known(harness)) {
      problem(`The settings enable a harness that does not exist: ${harness}.`);
      continue;
    }
    if (models.length === 0) routes.push({ name: named(harness, null), harness, model: null, about: "" });
    for (const model of models) routes.push({ name: named(harness, model), harness, model, about: "" });
  }
  return routes;
}

/** The same routes, checked against what each harness has: a model its harness does not have is dropped, and the rest say what they are for. */
export async function checked(routes: Route[], harnesses: Map<string, Harness>, problem: (text: string) => void): Promise<Route[]> {
  const has = new Map<string, Map<string, string>>();
  await Promise.all(
    [...new Set(routes.map((route) => route.harness))].map(async (harness) => {
      try {
        has.set(harness, new Map((await harnesses.get(harness)!.models()).map((model) => [model.id, model.about])));
      } catch (error) {
        // A harness that does not answer is not checked: its routes stay, and a turn on a wrong one fails by itself.
        problem(`${harness} did not say which models it has: ${error instanceof Error ? error.message : String(error)}`);
      }
    }),
  );
  return routes.flatMap((route) => {
    const models = has.get(route.harness);
    if (!models || route.model === null) return [route];
    if (!models.has(route.model)) {
      problem(`The settings enable ${route.name}, but ${route.harness} has no model ${route.model}. It has: ${[...models.keys()].join(", ")}.`);
      return [];
    }
    return [{ ...route, about: models.get(route.model) ?? "" }];
  });
}

/** The route of a name, when it is one that is enabled. This checks a decision and makes none: a name is a route or it
    is not, and nothing is picked in its place. */
export function find(routes: Route[], name: string): Route | undefined {
  const wanted = name.trim().toLowerCase();
  return routes.find((route) => route.name.toLowerCase() === wanted);
}

/** The user's rules, as written: `routing.md` in the first of the folders that has one. Nothing when none has. */
export function rules(...folders: string[]): string | null {
  for (const folder of folders) {
    const file = join(folder, "routing.md");
    if (!existsSync(file)) continue;
    try {
      return readFileSync(file, "utf8").trim() || null;
    } catch {
      return null;
    }
  }
  return null;
}

/** What Fast is told about the routes. Nothing when there is only one: then there is nothing to pick. */
export function told(routes: Route[], written: string | null): string {
  if (routes.length < 2) return "";
  const list = routes.map((route, index) => `${route.name}${index === 0 ? " (the default)" : ""}${route.about ? ` — ${route.about}` : ""}`).join("\n");
  return `# Routes

A new agent runs on a route: a harness and one of its models. \`spawn(task, route)\` names the route, as a string; without it the agent runs on the default route. These are all the routes there are:
${list}

- Pick the route by the user's rules below, by the kind of work asked for: a rename or a small edit is small on any page. No rule fits: no route, which is the default.
- The user names a harness or a model for a piece of work ("do this with Codex", "use the strongest one for this"): that wins over the rules, for that work. It still has to be one of the routes above.
- The user names one that is not among the routes ("do it with Gemini"): the script is one \`say\` that it is not enabled, and nothing else. No \`spawn\`, not on another route either and not to tell an agent so: the user decides what happens with the work.
- A route is written as it is above, whole. A name that is not one of them is an error, and nothing starts.
- Only \`spawn\` takes a route. \`send\` and \`fork\` take none: an agent stays on the route it was made on. Work that the user wants on another route than its agent's is a \`spawn\`, with what the new agent needs to know in the task.
- Never say a route's name unless the user asks which one something runs on.

${written ? `The user's rules:\n${written}` : "The user has written no rules: use the default unless the user names a route."}`;
}
