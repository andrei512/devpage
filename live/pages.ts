// The pages devpage serves, as the live session knows them.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

/** The map of the world: every page's address with its title. Built from the Markdown files devpage serves. */
export function map(root: string) {
  const found: { address: string; title: string }[] = [];
  const walk = (folder: string) => {
    for (const entry of readdirSync(join(root, folder), { withFileTypes: true })) {
      const path = `${folder}/${entry.name}`;
      if (entry.isDirectory()) {
        if (!entry.name.startsWith(".") && entry.name !== "node_modules") walk(path);
      } else if (entry.name.endsWith(".md") && entry.name !== "AGENTS.md" && entry.name !== "CLAUDE.md") {
        let title = "";
        try {
          title = /^#\s+(.+)$/m.exec(readFileSync(join(root, path), "utf8").slice(0, 4000))?.[1].trim() ?? "";
        } catch {}
        found.push({ address: path, title: title || entry.name });
      }
    }
  };
  walk("");
  return found.sort((a, b) => a.address.localeCompare(b.address));
}
