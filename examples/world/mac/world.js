import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { mkdtemp, readFile, rm, realpath } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const command = promisify(execFile);
const scripts = fileURLToPath(new URL("./scripts/", import.meta.url));

const mac = {
  openAddress: address => command("/usr/bin/open", [address]),
  openApp: app => command("/usr/bin/open", ["-a", app]),
  async run(script) {
    const folder = await realpath(scripts);
    const file = join(folder, script + ".applescript");
    if (await realpath(file) !== file) throw new Error("Use a script installed in the Mac package.");
    return (await command("/usr/bin/osascript", [file])).stdout.trim();
  },
  async screenshot() {
    const folder = await mkdtemp(join(tmpdir(), "devpage-screen-"));
    const file = join(folder, "screen.jpg");
    try {
      await command("/usr/sbin/screencapture", ["-x", "-t", "jpg", file]);
      return "data:image/jpeg;base64," + (await readFile(file)).toString("base64");
    } finally {
      await rm(folder, { recursive: true, force: true });
    }
  },
};

// Keeping the macOS calls together lets the protocol test use a known image
// without opening an application or capturing the user's screen.
export function registerMac({ action, kind, z }, overrides = {}) {
  const computer = { ...mac, ...overrides };
  kind("ran", {
    line: ({ what, text }) => `Mac: ${what}. ${text}`,
    image: fields => fields.image ?? null,
    wakes: (_, caller) => caller.who === "fast",
  });
  function register(name, description, input, perform) {
    action(name, { for: "both", description, input }, async (args, call) => {
      const result = await perform(args);
      call.add("ran", result);
      return result.image ? { text: result.text, image: result.image } : result.text;
    });
  }
  register("openAddress", "Open an HTTP or HTTPS address on the Mac.", { address: z.url({ protocol: /^https?$/ }) },
    async ({ address }) => { await computer.openAddress(address); return { what: "Open address", text: address }; });
  register("openApp", "Open a Mac application by name.", { app: z.string().min(1).regex(/^[^/\0]+$/) },
    async ({ app }) => { await computer.openApp(app); return { what: "Open app", text: app }; });
  register("run", "Run an AppleScript installed in the Mac package by its filename without the extension.", { script: z.string().regex(/^[A-Za-z0-9_-]+$/) },
    async ({ script }) => ({ what: `Run ${script}`, text: await computer.run(script) }));
  register("screenshot", "Take a screenshot of the Mac.", {},
    async () => ({ what: "Screenshot", text: "Captured the screen.", image: await computer.screenshot() }));
}

export default function register(part) {
  if (process.platform !== "darwin") throw new Error("The mac world part needs macOS.");
  registerMac(part);
}
