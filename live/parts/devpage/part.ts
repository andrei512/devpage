// The `devpage` part: the page the user is on, what is marked on it, and what Fast and the slow agents can do with it.
// It is registered like any part. What it needs of the service it is in, the user's open tab and devpage's comments,
// it is handed as its host.
import { z } from "zod";
import type { Later, Part as Declared, Registry } from "../../registry.ts";
import type { Part as Marked } from "../../tasks.ts";

/** The most of a page's text Fast is shown. */
const PAGE_TEXT = 14_000;
/** How long after a script asked the page to go somewhere a change of page is the script's doing, not the user's. */
const MOVING = 5000;

export interface Host {
  /** Asks the user's open page to do something, and waits for what it answers. Throws when no page is open or it does not answer. */
  ask: (action: Record<string, unknown>) => Promise<any>;
  /** The page in the user's tab: its address, and the files it is made of. Null when no tab is open. */
  tab: () => { address: string; sources: string[] } | null;
  /** Every page of the project, as its address and its title. */
  pages: () => { address: string; title: string }[];
  /** Saves a comment on a part of a page. */
  comment: (part: Marked, text: string) => Promise<unknown>;
  /** The part that was marked when a task was given to this agent; null when none was. Throws when the task is not the agent's. */
  marked: (agent: number, task: number) => Marked | null;
}

const flat = (text: string) => text.replace(/\s+/g, " ").trim();
const address = (source: string) => source.replace(/^\/*/, "/");

export function devpage(registry: Registry, host: Host) {
  let moving = 0;
  /** A page another part put over the user's: it is the page the user is on while it is there. */
  let cover: { address: string; dropped: () => void } | null = null;
  /** The functions of other parts after which the page is another one, as `devpage.go` is. */
  const changes = new Set(["devpage.go"]);

  const here = () => (cover ? { address: cover.address, sources: [cover.address] } : host.tab());
  const shows = (page: { sources: string[] } | null, part: Marked) => Boolean(page?.sources.some((source) => address(source) === address(part.page)));

  /** Brings the state in line with the page the user is on: a page the user opened becomes an entry, and a mark left on another page is dropped. */
  const settle = ({ state, add }: Later) => {
    const page = here();
    if (page && page.address !== state.address) {
      // A page that changed without a script having just asked for it is one the user opened themselves.
      if (state.address !== null && Date.now() > moving) add("page", { by: "me", text: `The user opened ${page.address}.` });
      // The page the script asked for has come: the next change is the user's again.
      moving = 0;
      state.address = page.address;
    }
    if (state.subject && page && !shows(page, state.subject)) mark(state, null, "user");
  };

  const mark = (state: Later["state"], part: Marked | null, by: "user" | "assistant") => {
    // A part of some other page than the one the user is on is never marked.
    if (part && !shows(here(), part)) part = null;
    state.subject = part;
    state.by = part ? by : null;
  };

  const step = <T>(change: (later: Later) => T) => registry.step("devpage", change);

  /** What the page answered an action with. Throws when the page could not do it. */
  const does = async (action: Record<string, unknown>) => {
    const done = await host.ask(action);
    if (done?.error) throw new Error(String(done.error));
    return done;
  };

  /** A quote names one place of the page Fast was shown. After a call that changes the page it is looked for when carried out. */
  const quoted = (quote: string, shown: Record<string, string>, earlier: { name: string }[]) => {
    if (earlier.some((call) => changes.has(call.name))) return;
    const text = /<<<\n([\s\S]*)\n>>>$/.exec(shown["The page's text"] ?? "")?.[1];
    if (text === undefined) throw new Error("no page is open");
    const all = flat(text);
    const first = all.indexOf(quote);
    if (first < 0) throw new Error(`the page has no text "${quote}". Copy the words exactly from the page text you were shown.`);
    if (all.indexOf(quote, first + 1) >= 0) throw new Error(`"${quote}" is on the page more than once. Use more words, so that it names one place.`);
  };
  const quote = z.string().transform(flat).pipe(z.string().min(1, "takes a few words of the page"));

  const part: Declared = {
    state: {
      address: z.string().nullable().default(null),
      subject: z.looseObject({ page: z.string(), quote: z.string() }).nullable().default(null),
      by: z.enum(["user", "assistant"]).nullable().default(null),
      panel: z.boolean().default(false),
    },
    entries: {
      page: { fields: { by: z.enum(["me", "fast"]), text: z.string() }, line: ({ text }) => `page  ${text}` },
    },
    functions: {
      go: {
        for: "fast",
        description: "an address from the list of pages, with an optional #heading",
        input: { address: z.string().trim().min(1).transform(address) },
        check: ({ address: wanted }, shown) => {
          const page = wanted.split("#")[0];
          const listed = (shown["The pages"] ?? "").split("\n").map((line) => line.split(" — ")[0]);
          if (!listed.includes(page)) throw new Error(`no page has the address ${page}. The addresses are in the list of pages.`);
        },
        run: async ({ address: wanted }, now) => {
          moving = Date.now() + MOVING;
          const went = await does({ name: "go", address: wanted });
          // Going to a page shows that page: what was over the user's page is dropped.
          if (cover) {
            const { dropped } = cover;
            cover = null;
            dropped();
            now.state.address = host.tab()?.address ?? null;
            mark(now.state, null, "user");
          }
          now.add("page", { by: "fast", text: `Fast opened ${wanted}.` });
          return went;
        },
      },
      scroll: {
        for: "fast",
        description: '"up", "down", "top", "bottom"',
        input: { where: z.enum(["up", "down", "top", "bottom"], 'takes "up", "down", "top" or "bottom"') },
        run: async ({ where }, now) => {
          const done = await does({ name: "scroll", to: where });
          now.add("page", { by: "fast", text: `Fast scrolled ${where === "top" || where === "bottom" ? `to the ${where}` : where}.` });
          return done;
        },
      },
      panel: {
        for: "fast",
        description: "the side panel with the tasks: true opens, false closes",
        input: { open: z.boolean("takes true to open the side panel, false to close it") },
        run: async ({ open }, now) => {
          const done = await does({ name: "panel", open });
          now.state.panel = open;
          now.add("page", { by: "fast", text: `Fast ${open ? "opened" : "closed"} the side panel.` });
          return done;
        },
      },
      point: {
        for: "fast",
        description: "scrolls to the part of the user's page with these words and highlights it",
        input: { quote },
        check: ({ quote }, shown, earlier) => quoted(quote, shown, earlier),
        run: async ({ quote }, now) => {
          const done = await does({ name: "point", quote });
          now.add("page", { by: "fast", text: `Fast pointed at ${JSON.stringify(quote)}.` });
          return done;
        },
      },
      select: {
        for: "fast",
        description: "marks that part",
        input: { quote },
        check: ({ quote }, shown, earlier) => quoted(quote, shown, earlier),
        run: async ({ quote }, now) => {
          const found = await does({ name: "find", quote });
          if (!found.part) throw new Error("the page has no such text");
          mark(now.state, found.part, "assistant");
          now.add("page", { by: "fast", text: `Fast marked ${JSON.stringify(quote)}.` });
          return { marked: true };
        },
      },
      write_comment: {
        for: "slow",
        tool: { always: true },
        description: "Saves a comment on the part that was marked when the task was given. Only when the user explicitly asked to leave a comment or a note there.",
        input: { task: z.number().describe("The task's number"), text: z.string().describe("The note, short and in the user's words") },
        run: async ({ task, text }, { caller }) => {
          if (caller.who !== "slow" || caller.agent === null) throw new Error(`Task ${task} is not yours.`);
          const marked = host.marked(caller.agent, task);
          if (!marked) throw new Error("no part was marked when this task was given: the user has to click or select one first");
          await host.comment(marked, text);
          return "Saved.";
        },
      },
    },
    addToTask: (_state, task) => (task.page ? [`The page the user is on: ${task.page}`, ...(task.marked ? [`Marked on it: "${task.marked}"`] : [])] : []),
    showFast: async () => {
      const pages = host.pages().map((page) => `${page.address} — ${page.title}`).join("\n");
      const read = await host.ask({ name: "read" }).catch(() => null);
      const state = step((later) => {
        settle(later);
        if (read) later.state.panel = read.panel === "open";
        return later.state;
      });
      if (typeof read?.address !== "string") return { "The page": "No page of the user's is open, or it does not answer.", "The page's text": "", "The pages": pages };
      const text = String(read.text ?? "");
      return {
        // A page over the user's answers with its own text, and is the page the user is on.
        "The page": `The user is on ${cover?.address ?? read.address}. ${state.subject ? `Marked now: ${JSON.stringify(state.subject.quote)}.` : "Nothing is marked."} The side panel is ${state.panel ? "open" : "closed"}.`,
        "The page's text": `${text.length > PAGE_TEXT ? `Its first ${PAGE_TEXT} characters:\n` : ""}<<<\n${text.slice(0, PAGE_TEXT)}\n>>>`,
        "The pages": pages,
      };
    },
  };
  void registry.part("devpage", part, undefined, import.meta.dir);

  return {
    /** The user's tab reported its page, or was closed. */
    settle: () => step(settle),
    /** A voice session came on: the page the user is on then is where the session starts, not a page the user opened. */
    arrive: () => step(({ state }) => void (state.address = here()?.address ?? null)),
    /** The user marked a part of the page, or cleared the mark. */
    marked: (marked: Marked | null) => step((later) => {
      settle(later);
      later.add("page", { by: "me", text: marked ? `The user marked ${JSON.stringify(marked.quote)}.` : "The user cleared the mark." });
      mark(later.state, marked, "user");
    }),
    /** Nothing is marked any more: a session starts or ends with nothing marked. */
    clear: () => step(({ state }) => mark(state, null, "user")),
    /** A task was given with this part: the mark went with it, and the next command starts with nothing marked. */
    gave: (given: Marked) => step(({ state }) => {
      if (JSON.stringify(state.subject) === JSON.stringify(given)) mark(state, null, "user");
    }),
    /** Where the user is now and what is marked: what a message is stored with. */
    stamp: () => step((later) => {
      settle(later);
      return { page: here()?.address ?? null, part: later.state.subject as Marked | null };
    }),
    /** Another part puts a page of its own over the user's. `dropped` is called when `devpage.go` takes it away. */
    cover: (over: string, dropped: () => void) => {
      cover = { address: over, dropped };
      // A mark left on the page under it is not about it.
      step(({ state }) => {
        state.address = over;
        if (state.subject && !shows(here(), state.subject)) mark(state, null, "user");
      });
    },
    /** The page over the user's is gone: the user's own page is the page the user is on again. */
    uncover: () => {
      cover = null;
      step(({ state }) => { state.address = here()?.address ?? null; mark(state, null, "user"); });
    },
    /** A function of another part after which the page is another one: a quote in the same script is then looked for when carried out. */
    changesPage: (name: string) => void changes.add(name),
  };
}

export type Devpage = ReturnType<typeof devpage>;
