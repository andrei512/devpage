// The `canvas` part: a large sheet over the user's page that shows one scratch pad, a Markdown file that is not in the
// project. It needs the `devpage` part: the pad on the canvas is the page the user is on while it is open.
import { existsSync, readFileSync, readdirSync, statSync, watch, writeFileSync } from "node:fs";
import { join } from "node:path";
import { z } from "zod";
import type { Part as Declared, Registry } from "../../registry.ts";
import type { Devpage } from "../devpage/part.ts";

/** A pad's address in the session: `pads/` and its file's name. No page of the project has an address without a `/` in front. */
const PAD = /^pads\/[a-z0-9][a-z0-9-]*\.md$/;

/** Added to a task that names a scratch pad, so the order of drawing is in front of the agent when it starts, whatever its conversation holds. */
const PAD_ORDER = `When this task has you draw a scratch pad or a new part of one, the order is not optional: right after your first update and before you read anything, save the pad as its title and one \`Soon\` widget for each part you plan, written as a tag such as \`<Soon kind="list" height="m" />\`, alone on its line. These widgets are the only way progress is shown on a pad: never the word "Soon", a heading for a part that is not drawn yet, or any other text that says something is coming. Then put the real part in the place of each, one at a time from the top, with one save for each. Never write a pad in one save.`;

/** A task about a scratch pad: Fast names the pad's address, or it is the page the user is on. */
const aboutPad = (text: string) => /(^|[^\w/.-])pads\/[a-z0-9][a-z0-9-]*\.md\b/.test(text);

/** A scratch pad's address is made from its idea's title, so the same idea is the same pad. Null for a title with no letter or digit in it. */
export function padOf(title: string) {
  const slug = title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 60);
  return slug ? `pads/${slug}.md` : null;
}

export interface Host {
  /** The folder the scratch pads are in. */
  folder: string;
  log: (kind: string, text: string) => void;
}

export function canvas(registry: Registry, devpage: Devpage, host: Host) {
  const file = (address: string) => join(host.folder, address.slice("pads/".length));
  /** The titles of the pads a script named, until the pad is made. */
  const titles = new Map<string, string>();

  /** The scratch pads there are, newest first, each with its idea's title: its first heading. */
  const pads = () => {
    try {
      return readdirSync(host.folder)
        .filter((name) => PAD.test(`pads/${name}`))
        .map((name) => {
          const title = /^#\s+(.+)$/m.exec(readFileSync(join(host.folder, name), "utf8").slice(0, 2000))?.[1].trim() ?? name;
          return { address: `pads/${name}`, title, changed: statSync(join(host.folder, name)).mtimeMs };
        })
        .sort((a, b) => b.changed - a.changed)
        .slice(0, 30);
    } catch {
      return [];
    }
  };

  /** The canvas was closed: by a script, or because `devpage.go` or a pad that is gone took the pad away. */
  const closed = (text: string) => registry.step("canvas", ({ state, add }) => {
    if (!state.canvas) return false;
    state.canvas = null;
    add("canvas", { text });
    return true;
  });
  /** The pad is the page the user is on while the canvas is open. `devpage.go` shows the page it goes to, so it closes the canvas. */
  const cover = (address: string) => devpage.cover(address, () => closed("Fast closed the canvas."));

  const part: Declared = {
    state: {
      /** The scratch pad on the canvas, with a number that changes whenever its file does; null while the canvas is closed. */
      canvas: z.object({ pad: z.string().regex(PAD), version: z.number() }).nullable().default(null),
    },
    entries: {
      canvas: { fields: { text: z.string() }, line: ({ text }) => `canvas  ${text}` },
    },
    functions: {
      pad: {
        for: "fast",
        description: "the address of the scratch pad for an idea, from its title in a few words",
        input: { title: z.string().transform((title) => title.replace(/\s+/g, " ").trim()) },
        check: ({ title }) => {
          const address = padOf(title);
          if (!address) throw new Error("takes the idea's title, in a few words");
          return address;
        },
        run: ({ title }) => {
          const address = padOf(title);
          if (address) titles.set(address, title);
          return address;
        },
      },
      show: {
        for: "fast",
        description: "opens the canvas on that scratch pad, over the user's page",
        input: { address: z.string().transform((address) => address.trim().replace(/^\/*/, "")) },
        check: ({ address }, shown, earlier) => {
          const listed = (shown["The scratch pads"] ?? "").split("\n").map((line) => line.split(" — ")[0]);
          const made = earlier.some((call) => call.name === "canvas.pad" && padOf(String(call.input.title)) === address);
          if (!made && !listed.includes(address)) throw new Error(`there is no scratch pad ${address}. Show one from the list of scratch pads, or make a new one: canvas.show(canvas.pad("The idea's title")).`);
        },
        run: ({ address }, now) => {
          if (!PAD.test(address)) throw new Error(`${address} is not a scratch pad`);
          // A pad that is not there yet is made, with its idea's title, so there is something to show at once.
          if (!existsSync(file(address))) writeFileSync(file(address), `# ${titles.get(address) ?? address.slice("pads/".length, -3).replace(/-/g, " ")}\n`);
          titles.delete(address);
          now.state.canvas = { pad: address, version: Date.now() };
          cover(address);
          now.add("canvas", { text: `Fast opened the canvas on the scratch pad ${address}.` });
          return { canvas: address };
        },
      },
      close: {
        for: "fast",
        description: "closes the canvas: the user's page is there as the user left it",
        run: (_input, now) => {
          if (!now.state.canvas) return { closed: false };
          now.state.canvas = null;
          devpage.uncover();
          now.add("canvas", { text: "Fast closed the canvas." });
          return { closed: true };
        },
      },
    },
    widgets: { DevpageLiveCanvas: "float" },
    addToTask: (_state, task) => (aboutPad(task.prompt) || aboutPad(task.page ?? "") ? [PAD_ORDER] : []),
    showFast: (state) => ({
      "The canvas": state.canvas ? `The canvas is open: the user is looking at the scratch pad ${state.canvas.pad}.` : "",
      "The scratch pads": pads().map(({ address, title }) => `${address} — ${title}`).join("\n"),
    }),
  };
  void registry.part("canvas", part, undefined, import.meta.dir);
  // A script that showed or closed the canvas changed the page: a quote after it is looked for when carried out.
  devpage.changesPage("canvas.show");
  devpage.changesPage("canvas.close");

  // The canvas that was open when the program last ended is open again, when its pad is still there.
  const open = registry.state("canvas").canvas as { pad: string } | null;
  if (open && PAD.test(open.pad) && existsSync(file(open.pad))) cover(open.pad);
  else if (open) registry.step("canvas", ({ state }) => void (state.canvas = null));

  // The pad on the canvas is drawn again when its file changes: devpage does not watch a file outside the project.
  let changed: ReturnType<typeof setTimeout> | undefined;
  let watching: ReturnType<typeof watch> | undefined;
  try {
    watching = watch(host.folder, (_event, name) => {
      const shown = registry.state("canvas").canvas as { pad: string } | null;
      if (!shown || (name && `pads/${name}` !== shown.pad)) return;
      clearTimeout(changed);
      changed = setTimeout(() => {
        const now = registry.state("canvas").canvas as { pad: string } | null;
        if (!now) return;
        if (existsSync(file(now.pad))) return registry.step("canvas", ({ state }) => void (state.canvas = { pad: now.pad, version: Date.now() }));
        if (closed("Fast closed the canvas.")) devpage.uncover();
      }, 150);
    });
  } catch (error) {
    host.log("pads_not_watched", error instanceof Error ? error.message : String(error));
  }

  return {
    /** The user opened the canvas on a pad: by a click on a task whose change is on it. */
    opened: (address: string) => {
      if (!PAD.test(address) || !existsSync(file(address))) throw new Error(`there is no scratch pad ${address}`);
      registry.step("canvas", ({ state, add }) => {
        state.canvas = { pad: address, version: Date.now() };
        add("canvas", { text: `The user opened the canvas on the scratch pad ${address}.` });
      });
      cover(address);
    },
    /** The user closed the canvas. */
    closed: () => {
      if (closed("The user closed the canvas themselves.")) devpage.uncover();
    },
    /** Stops watching the folder of the pads. */
    end: () => {
      clearTimeout(changed);
      watching?.close();
    },
    /** A pad's Markdown, for devpage to draw it as a page; null when there is no such pad. */
    markdown: (address: string) => (PAD.test(address) && existsSync(file(address)) ? readFileSync(file(address), "utf8") : null),
  };
}
