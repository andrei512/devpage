// An agent's prompt is put together here, from the core's template for that agent in `prompts/` and the text each
// part has for it. The text is placed by its headings and never read for what it means. Run as a command,
// `bun prompt.ts <voice|fast|slow>`, this prints the prompt the program's own parts give, with a line before each
// piece that says where it came from.
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

export type Agent = "voice" | "fast" | "slow";

/** A part that may have text for the agents: `voice.md`, `fast.md`, `slow.md` in its folder. */
export interface TextOf {
  part: string;
  folder: string;
}

/** A piece of a prompt, and where its words are written. */
export interface Piece {
  from: string;
  text: string;
}

/** What fills a `{{name}}` of a template: its text, and where that came from when it is not written in the core. */
export type Values = Record<string, string | { from: string; text: string }>;

const CORE = join(import.meta.dir, "prompts");

const read = (file: string) => readFileSync(file, "utf8").replace(/\n+$/, "");
const slot = (line: string) => /^\{\{(\w+)\}\}$/.exec(line)?.[1];

interface Section {
  heading: string | null;
  text: string;
}

/** A text as its sections: what stands before the first `# ` heading, then each heading with what is under it. A `{{name}}` alone on a line, outside a code block, is a section of its own. */
function sections(text: string): (Section | { slot: string })[] {
  const found: (Section | { slot: string })[] = [];
  let open: { heading: string | null; lines: string[] } = { heading: null, lines: [] };
  let fenced = false;
  const close = () => {
    const written = open.lines.join("\n").replace(/^\n+|\n+$/g, "");
    if (written) found.push({ heading: open.heading, text: written });
  };
  for (const line of text.split("\n")) {
    if (line.startsWith("```")) fenced = !fenced;
    const name = fenced ? undefined : slot(line);
    if (name) {
      close();
      found.push({ slot: name });
      open = { heading: null, lines: [] };
    } else if (!fenced && line.startsWith("# ")) {
      close();
      open = { heading: line.slice(2).trim(), lines: [line] };
    } else {
      open.lines.push(line);
    }
  }
  close();
  return found;
}

/** The agent's prompt as its pieces, in order. Joined, their texts are the prompt. */
export function assemble(agent: Agent, parts: TextOf[], values: Values = {}): Piece[] {
  const given = (name: string) => {
    const value = values[name];
    return value === undefined ? undefined : typeof value === "string" ? { from: name, text: value } : value;
  };
  const inline = (text: string) => text.replace(/\{\{(\w+)\}\}/g, (whole, name) => given(name)?.text ?? whole);

  // What each part adds: under a heading of the template, or as a section of its own.
  const template = sections(read(join(CORE, `${agent}.md`)));
  const headings = new Set(template.flatMap((item) => ("heading" in item && item.heading ? [item.heading] : [])));
  // A label is a line of the template that is a name and a colon alone, such as GPT-Live's `Backend tools:`: a part adds
  // to it with a heading of that name, and its lines go at the end of the list under the label.
  const labels = new Set(template.flatMap((item) => ("text" in item ? item.text.split("\n").filter((line) => /^[^\s-].*:$/.test(line)).map((line) => line.slice(0, -1)) : [])));
  const added = new Map<string, Piece[]>();
  const listed = new Map<string, Piece[]>();
  const own: Piece[] = [];
  for (const { part, folder } of parts) {
    const file = join(folder, `${agent}.md`);
    if (!existsSync(file)) continue;
    for (const item of sections(read(file))) {
      if ("slot" in item) throw new Error(`${file}: {{${item.slot}}} alone on a line is the template's, not a part's`);
      const from = `${part}: ${agent}.md${item.heading ? ` · ${item.heading}` : ""}`;
      const under = () => inline(item.text.slice(item.text.indexOf("\n") + 1).replace(/^\n+/, ""));
      if (item.heading && headings.has(item.heading)) {
        added.set(item.heading, [...(added.get(item.heading) ?? []), { from, text: under() }]);
      } else if (item.heading && labels.has(item.heading)) {
        listed.set(item.heading, [...(listed.get(item.heading) ?? []), { from, text: under() }]);
      } else {
        own.push({ from, text: inline(item.text) });
      }
    }
  }

  const pieces: Piece[] = [];
  /** A piece follows the one before it after an empty line. A list item that follows a list item goes on the same list. */
  const put = (piece: Piece, sameList = false) => {
    const before = pieces.at(-1)?.text ?? "";
    const list = sameList && before.split("\n").at(-1)!.startsWith("- ") && piece.text.startsWith("- ");
    pieces.push({ from: piece.from, text: `${pieces.length ? (list ? "\n" : "\n\n") : ""}${piece.text}` });
  };
  for (const item of template) {
    if ("slot" in item) {
      if (item.slot === "shared") put({ from: "core: shared.md", text: read(join(CORE, "shared.md")) });
      else if (item.slot === "parts") for (const piece of own) put(piece);
      else {
        const value = given(item.slot);
        if (value?.text) put(value);
      }
      continue;
    }
    // A `{{name}}` alone on a line inside the section, as in a code block, is a piece of its own: it has its own source.
    const from = `core: ${agent}.md${item.heading ? ` · ${item.heading}` : ""}`;
    let lines: string[] = [];
    let first = true;
    const flush = () => {
      if (!lines.length) return;
      const text = inline(lines.join("\n"));
      if (first) put({ from, text });
      else pieces.push({ from, text: `\n${text}` });
      first = false;
      lines = [];
    };
    /** The label whose list is being read: what parts add to it goes after its last item. */
    let label: string | null = null;
    const close = () => {
      if (!label) return;
      flush();
      for (const piece of listed.get(label) ?? []) pieces.push({ from: piece.from, text: `\n${piece.text}` });
      label = null;
    };
    for (const line of item.text.split("\n")) {
      if (label && !line.startsWith("- ") && !(line === "" && lines.at(-1) === `${label}:`)) close();
      if (line.endsWith(":") && listed.has(line.slice(0, -1))) label = line.slice(0, -1);
      const value = slot(line) ? given(slot(line)!) : undefined;
      if (!value || first && !lines.length) { lines.push(line); continue; }
      flush();
      pieces.push({ from: value.from, text: `\n${value.text}` });
    }
    close();
    flush();
    for (const piece of (item.heading && added.get(item.heading)) || []) put(piece, true);
  }
  return pieces;
}

/** The agent's prompt, as the agent gets it. */
export function prompt(agent: Agent, parts: TextOf[], values: Values = {}) {
  return assemble(agent, parts, values).map((piece) => piece.text).join("");
}

if (import.meta.main) {
  const agent = process.argv[2];
  if (agent !== "voice" && agent !== "fast" && agent !== "slow") {
    console.error("bun prompt.ts <voice|fast|slow>");
    process.exit(1);
  }
  // The registry with the core and the program's two parts, as a session has it. Nothing here is asked of a page.
  const { mkdtempSync, rmSync } = await import("node:fs");
  const { tmpdir } = await import("node:os");
  const { Registry } = await import("./registry.ts");
  const { registerCore } = await import("./core-parts.ts");
  const { devpage } = await import("./parts/devpage/part.ts");
  const { canvas } = await import("./parts/canvas/part.ts");
  const none = () => { throw new Error("no page is open"); };
  const pads = mkdtempSync(join(tmpdir(), "devpage-prompt-"));
  const registry = new Registry(":memory:");
  registerCore(registry);
  const page = devpage(registry, { ask: none, tab: () => null, pages: () => [], comment: none, marked: none });
  const shown = canvas(registry, page, { folder: pads, log: () => {} });
  const pieces =
    agent === "fast" ? (await import("./fast.ts")).pieces(registry)
    : agent === "slow" ? (await import("./agents.ts")).pieces(registry.texts(), "<the folder of the scratch pads>", false)
    : assemble("voice", registry.texts());
  for (const piece of pieces) console.log(`── ${piece.from}\n${piece.text.replace(/^\n+/, "")}`);
  shown.end();
  rmSync(pads, { recursive: true });
}
