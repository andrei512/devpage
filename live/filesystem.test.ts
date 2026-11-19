// Resolve the actual launch options offline with Codex. No login or model turn is needed.
import { expect, test } from "bun:test";
import { mkdtempSync, mkdirSync, realpathSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { filesystemArgs } from "./codex.ts";

test.skipIf(!Bun.which("codex"))("Codex resolves project-only and full filesystem profiles", () => {
  const fixture = realpathSync(mkdtempSync(join(tmpdir(), "live-permissions-")));
  try {
    const project = join(fixture, "project");
    const pads = join(fixture, "scratch pads");
    const home = join(fixture, "codex-home");
    for (const dir of [project, pads, home]) mkdirSync(dir);
    for (const fullFilesystemAccess of [false, true]) {
      const args = filesystemArgs({ fullFilesystemAccess, also: [pads] });
      const result = Bun.spawnSync(["codex", "debug", "prompt-input", "-c", "project_doc_max_bytes=0", ...args], {
        cwd: project, env: { ...process.env, CODEX_HOME: home },
      });
      expect(result.stderr.toString()).not.toContain("Error:");
      expect(result.exitCode).toBe(0);
      const prompt = result.stdout.toString();
      expect(prompt.includes("<special>:root</special>")).toBe(fullFilesystemAccess);
      expect(prompt.includes("<special>:minimal</special>")).toBe(!fullFilesystemAccess);
      expect(prompt.includes(`<path>${project}</path>`)).toBe(!fullFilesystemAccess);
      expect(prompt.includes(`<path>${pads}</path>`)).toBe(!fullFilesystemAccess);
    }
  } finally {
    rmSync(fixture, { recursive: true, force: true });
  }
});
