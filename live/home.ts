// Where the live session keeps things on this machine. Nothing of a session is kept in the project it is about: its
// folder is in devpage's own folder in the home directory, with the settings that hold for every project.
import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

/** devpage's folder on this machine. */
export const HOME = join(homedir(), ".devpage");

/** The folder of one project's session: the log, the world, the tasks, the agents, the scratch pads. It is named after
    the project's path as Claude Code names a project's folder: every character that is not a letter or a digit is a `-`. */
export const stateOf = (root: string) => join(HOME, "live", "projects", root.replace(/[^a-zA-Z0-9]/g, "-"));

/** The settings for every project: what is under `live` in devpage's `config.yml`. Nothing when there is none, or the file does not read. */
export function everywhere(problem: (text: string) => void): Record<string, unknown> {
  const file = join(HOME, "config.yml");
  if (!existsSync(file)) return {};
  try {
    const live = (Bun.YAML.parse(readFileSync(file, "utf8")) as any)?.live;
    return live && typeof live === "object" && !Array.isArray(live) ? live : {};
  } catch (error) {
    problem(`${file} does not read: ${error instanceof Error ? error.message : String(error)}`);
    return {};
  }
}

/** The settings of this project: `extensions.live.config` of its document file, as devpage passes it. */
export function here(problem: (text: string) => void): Record<string, unknown> {
  try {
    const config = JSON.parse(process.env.DEVPAGE_CONFIG || "null");
    return config && typeof config === "object" && !Array.isArray(config) ? config : {};
  } catch (error) {
    problem(`The project's settings do not read: ${error instanceof Error ? error.message : String(error)}`);
    return {};
  }
}
