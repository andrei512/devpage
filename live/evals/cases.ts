// The cases of Fast's eval: see ../eval.ts. They are written on a sample session, sessions/sample.json: a session
// written for the eval, about an invented project, Kelpie, a small command-line backup tool with a docs site. Nothing
// in it is a recording, and nothing in it is taken from a real conversation or a real project. The cases of the set
// `sample` are wake-ups of that written session, with what a right script does there. The other ones go on after one
// of its wake-ups with things the user could say next. A moment where Fast did the wrong thing in real use becomes a
// case here as a new moment on the sample project that asks for the same behavior.
import type { Case, Expect } from "../eval.ts";
import { told as told_routes } from "../routes.ts";

const SESSION = "sample";
const FRONT = "/README.md";
const PRUNE = "/commands/prune.md";
const RETENTION = "/internals/retention.md";

const sample = (run: number, name: string, why: string, expect: Expect): Case => ({ name: `sample-${String(run).padStart(2, "0")}-${name}`, why, session: SESSION, run, expect });

/** After wake-up 22 the user is on the retention page with the panel open. a1 knows how prune counts a snapshot that two rules keep, and reworded the worked example; a2 knows how prune and snap lock the store. Nothing runs. */
const after = (name: string, why: string, steps: Case["steps"], expect: Expect, wake = 22): Case => ({ name, why, session: SESSION, after: wake, steps, expect });

const NOTHING: Expect = { does: [] };
/** A judgment is not Fast's to give: the user hears at most that it is being thought about. */
const NO_OPINION = "It gives no opinion, assessment or recommendation of its own: at most it says that it is looking into it or thinking about it.";
/** An agent's answer reaches the user once, with nothing added. */
const told = (what: string): Expect => ({ does: [{ do: "say" }], line: [what] });

/** Giving work to an agent, whichever way. */
const GIVES: ("spawn" | "send" | "fork")[] = ["spawn", "send", "fork"];

// Pieces of history that several cases start from. The first thing the user says after wake-up 22 is m47; a line
// Fast speaks takes the next number, and the first update is u14. The next new agent is a3.
/** Work for a new agent, a3. */
const EXAMPLES = { he: "Run the examples on the quick start page and tell me whether they all still work", script: 'spawn(`The user said: "${m47}"`);\nsay("I\'m running them.");' };
const EXAMPLES_FAIL = { updates: [{ agent: 3, type: "finished" as const, text: "Two of the 6 examples on the quick start page fail: the restore one, and the one that uses an ignore file. I changed nothing." }], script: "say(u14);" };
/** Work for a1, which is then busy. */
const SECOND_EXAMPLE = { he: "Add a second worked example under this one, with monthly rules as well", script: 'a1.send(`The user said: "${m47}"`);\nsay("On it.");' };
/** a1 is busy with the second example, and a3 runs the quick start's examples. */
const BOTH_BUSY = [SECOND_EXAMPLE, { he: "And run the examples on the quick start page to see whether they still work", script: 'spawn(`The user said: "${m49}"`);\nsay("I\'m running them.");' }];
/** Both asked the user something, and the user heard both questions. */
const BOTH_ASKED = [...BOTH_BUSY, { updates: [{ agent: 1, type: "question" as const, text: "Should the second example go above the first one or below it?" }, { agent: 3, type: "question" as const, text: "With the released binary, or with a build from main?" }], script: "say(`${u14} And: ${u15}`);" }];
/** A new agent, a3, shortens the opening and asks about the sizes. */
const SHORTER_OPENING = [
  { he: "Make the opening of this page shorter", script: 'spawn(`The user said: "${m47}"`);\nsay("On it.");' },
  { updates: [{ agent: 3, type: "question" as const, text: "Should the sizes of the test store stay in the opening, or move down to the worked example?" }], script: "say(u14);" },
];

/** A scratch pad that is there, with its text as the user sees it. */
const SNAP = "pads/what-happens-during-a-snap.md";
const SNAP_PAD = {
  [SNAP]: {
    title: "What happens during a snap",
    text: "What happens during a snap Kelpie reads the folder, stores what is new, and then writes the snapshot. One run 1. Walk the folder and leave out what the ignore rules name. 2. Split every changed file into chunks. 3. Upload the chunks the store does not have yet. 4. Write the snapshot, which lists every file and its chunks. 5. Release the store's lock.",
  },
};
/** The script that opened the canvas on that pad and gave an agent the drawing. */
const DRAW = { he: "Show me what happens during a snap", script: 'const pad = canvas.pad("What happens during a snap");\ncanvas.show(pad);\nspawn(`The user said: "${m47}"\\nDraw the answer on the scratch pad ${pad}, which the user is looking at now.`);\nsay("I\'m drawing it.");' };
/** The agent drew it, and the user heard so. */
const DRAWN = [DRAW, { updates: [{ agent: 3, type: "finished" as const, text: "The pad shows one run of a snap from start to end, in five steps." }], script: "say(`${u14}`);" }];
/** A written moment about the canvas, with the scratch pads there are. */
const canvas = (name: string, why: string, steps: Case["steps"], expect: Expect, pads: Case["pads"] = {}): Case => ({ ...after(name, why, steps, expect), pads });

/** Three routes on two harnesses, and rules for two kinds of work. */
const ROUTES = told_routes(
  [
    { name: "claude-code/fable", harness: "claude-code", model: "fable", about: "Fable 5.2: For your toughest challenges" },
    { name: "claude-code/haiku", harness: "claude-code", model: "haiku", about: "Haiku 5.5: Fastest for quick answers" },
    { name: "codex/gpt-6.2-astra", harness: "codex", model: "gpt-6.2-astra", about: "GPT-6.2-Astra: Our most capable model for complex coding and professional work." },
  ],
  "# Routing\n\n- Small edits, renames, quick lookups: claude-code/haiku.\n- Building or changing code: codex/gpt-6.2-astra.",
);
const routed = (name: string, why: string, he: string, expect: Expect): Case => ({ ...after(name, why, [{ he }], expect), routes: ROUTES });

/** What each set is for. A case is in the set it is written under; `held-out` is not part of an everyday run. */
export const SETS: Record<string, string> = {
  "routing": "Which harness and model a new agent runs on.",
  "sample": "Wake-ups of the sample session, as it is written.",
  "follow-ups": "Which agent gets a follow-up.",
  "reports": "What the user hears of what the agents report.",
  "answers": "What Fast answers by itself.",
  "judgment": "A judgment is not Fast's to give.",
  "nothing": "Words that call for nothing.",
  "page": "Moving and pointing on the user's page.",
  "work": "Work that goes to an agent.",
  "held-out": "Never used to change the brief: for the last check of a change, not for everyday runs.",
  "red-team": "Hard moments written to break it.",
  "scratch-pad": "When a scratch pad is shown, and how an open one is used.",
  "acknowledgments": "How much the user hears when work is taken on.",
};

/** The cases of one set. */
const set = (name: keyof typeof SETS, cases: Case[]): Case[] => cases.map((one) => ({ ...one, sets: [...(one.sets ?? []), name] }));

export const CASES: Case[] = [
  ...set("routing", [
    routed("route-by-rule-small", "A rule names the route for a small edit.", "Rename the heading Notes on the prune page to Good to know", { does: [{ do: "spawn", route: "claude-code/haiku" }], may: ["say"] }),
    routed("route-by-rule-code", "A rule names the route for code.", "Write a small script that checks that every command page has a section on exit codes", { does: [{ do: "spawn", route: "codex/gpt-6.2-astra" }], may: ["say"] }),
    routed("route-default", "No rule fits: the default route, named or not.", "Go through the whole changelog and the troubleshooting page and work out which kinds of trouble came up most since version 0.8, then tell me", { does: [{ do: "spawn", route: ["", "claude-code/fable"] }], may: ["say"] }),
    routed("route-by-voice", "What the user names wins over the rule for a small edit.", "Use Codex for this one: rename the heading Notes on the prune page to Good to know", { does: [{ do: "spawn", route: "codex/gpt-6.2-astra" }], may: ["say"] }),
    routed("route-not-enabled", "A harness that is not among the routes starts nothing.", "Proofread the troubleshooting page for me, and do it with Gemini", { does: [{ do: "say" }], line: ["It says that Gemini is not enabled, or not available, and does not say the work has started."] }),
    routed("route-follow-up-stays", "A follow-up takes no route: it goes to the agent that knows it.", "Actually, put the old sentence back", { does: [{ do: ["send", "fork"], agent: 1 }], may: ["say"] }),
  ]),

  ...set("sample", [
    sample(1, "where-am-i", "A question the page's text answers is answered at once.", { does: [{ do: "say" }], line: ["It says that the page the user is on is the front page of Kelpie's docs."] }),
    sample(2, "can-you-see-the-page", "A question about itself is answered without claiming what it cannot do.", { does: [{ do: "say" }], line: ["It says yes: it gets the text of the page the user is on.", "It claims no sight beyond what it is shown: the page's address and text, the marked part, the side panel."] }),
    sample(3, "open-a-page-by-its-subject", "A page the user names by what it is about is found in the list of pages.", { does: [{ do: "go", address: RETENTION }], may: ["say"] }),
    sample(4, "scroll-to-a-mark-that-is-not-there", "The user asks for something that cannot be seen: the page moves or the user is told, and nothing is made up.", { does: [], may: ["scroll", "say"], line: ["It does not speak as if a sentence were marked: it says nothing is marked, or only that it scrolled."] }),
    sample(5, "is-anything-marked", "Says what it was shown, not what the user expects.", { does: [{ do: "say" }], may: ["spawn"], line: ["It says that what it is shown has nothing marked."] }),
    sample(6, "question-in-two-pieces", "A question said in two pieces that has to be read in the code goes to an agent whole.", { does: [{ do: GIVES }], may: ["say"], prompt: ["It has both of the user's messages: the nine snapshots on the laptop, and the question.", "It asks how prune counts a snapshot that fits two rules."], line: ["It gives no finished answer on how such a snapshot is counted."] }),
    sample(8, "finished-with-a-guess", "An answer that has a guess in it is passed on as a guess.", told("It says a snapshot that two rules keep is counted once, and gives this as the probable reason for the nine, not as a certain one.")),
    sample(9, "follow-up-goes-to-the-same-agent", "A follow-up on what a1 just answered goes to a1, not to a new agent.", { does: [{ do: ["send", "fork"], agent: 1 }], may: ["say"], prompt: ["It has the user's message.", "It asks whether the worked example on the retention page is wrong."] }),
    sample(10, "finished-with-its-doubt", "The answer reaches the user with its doubt.", told("It says the example is most likely wrong: 9 snapshots are left, not 11. It may say how the sentence could read. It does not say anything was changed.")),
    sample(12, "scroll", "A plain page command is done without a word.", { does: [{ do: "scroll", where: "down" }], may: ["scroll"] }),
    sample(13, "scroll-again-impatient", "The user asks again, and says so: the page moves, and nothing is given to an agent.", { does: [{ do: "scroll", where: "down" }], may: ["scroll", "say"] }),
    sample(14, "question-to-read", "A question that has to be read in the project goes to an agent, with a short line.", {
      does: [{ do: "spawn" }, { do: "say" }],
      prompt: ["It has the user's whole message, both questions."],
      line: ["It says the question is being looked into, and gives no finished answer on whether prune can remove those chunks or takes a lock."],
    }),
    sample(16, "finished-long-answer", "A long finished answer is spoken, shorter, with nothing added.", told("It gives the substance of the agent's answer: that an unfinished snapshot's chunks are not removed, the lock or the three steps, and at least one risk or suggestion.")),
    sample(17, "did-you-read-the-code", "A question about itself is answered without claiming what it cannot do.", { does: [{ do: "say" }], line: ["It does not say or imply that the dispatcher itself reads the project's code or files: the code was read by an agent, or for it."] }),
    sample(20, "that-sentence", "\"That sentence\" is the one a1 said could be reworded, two answers back: the agent is told so.", { does: [{ do: ["send", "fork"], agent: 1 }], may: ["say"], prompt: ["It has the user's message.", "It makes clear, or what that agent said before makes clear, that the sentence to fix is the one of the worked example that says 11 snapshots are left."] }),
    sample(22, "finished-a-change", "What was done is told as the agent told it.", told("It says the sentence under the worked example was changed, and what it says now or that the old wording can be got back.")),
  ]),

  ...set("follow-ups", [
    // Follow-ups: which agent
    after("follow-up-put-it-back", "\"The old sentence\" is the one a1 reworded.", [{ he: "Actually, put the old sentence back" }], { does: [{ do: ["send", "fork"], agent: 1 }], may: ["say"], prompt: ["It has the user's message."] }),
    after("follow-up-undo-that", "\"That\" is the last thing done, by a1.", [{ he: "Undo that" }], { does: [{ do: ["send", "fork"], agent: 1 }], may: ["say"], prompt: ["It has the user's message.", "It makes clear, or the agent's own work makes clear, that what is undone is the change to the worked example."] }),
    after("follow-up-more-on-the-finding", "Work on what a1 found goes to a1 or to its fork.", [{ he: "Okay, then put a second example under it that has monthly rules too" }], { does: [{ do: ["send", "fork"], agent: 1 }], may: ["say"], prompt: ["It has the user's message."] }),
    after("follow-up-older-answer", "A follow-up on the answer about the lock goes to the agent that gave it, though another agent worked since.", [{ he: "And those ten minutes that prune waits, can I set that to something shorter" }], { does: [{ do: ["send", "fork"], agent: 2 }], may: ["say"], prompt: ["It has the user's message."] }),
    after("new-work-new-agent", "Work no agent knows gets a new agent.", [{ he: EXAMPLES.he }], { does: [{ do: "spawn" }], may: ["say"], prompt: ["It has the user's message.", "It asks for the examples to be run and reported on, and for nothing to be changed."] }),
    after("busy-agent-related", "More on what a busy agent is doing goes to that agent.", [SECOND_EXAMPLE, { he: "And use the same sixty nights in it as the first one does" }], { does: [{ do: "send", agent: 1 }], may: ["say"], prompt: ["It has the user's message."] }),
    after("busy-agent-unrelated", "Separate work does not wait behind a busy agent.", [SECOND_EXAMPLE, { he: "Meanwhile, run the examples on the quick start page and see whether they still work" }], { does: [{ do: "spawn" }], may: ["say"], prompt: ["It has the user's message."] }),
  ]),

  ...set("reports", [
    // What the user hears of the agents
    after("agent-asks", "An agent's question is put to the user as a question.", [SHORTER_OPENING[0], { updates: SHORTER_OPENING[1].updates }], { does: [{ do: "say" }], line: ["It asks the user whether the sizes of the test store stay in the opening or move down to the worked example."] }),
    after(
      "agent-failed",
      "A failure is told plainly, with its reason.",
      [EXAMPLES, { updates: [{ agent: 3, type: "failed", text: "I could not run the examples: there is no kelpie binary on this machine, and I may not install one." }] }],
      { does: [{ do: "say" }], may: ["spawn", "send"], line: ["It says the examples were not run, and why."] },
    ),
    after(
      "two-answers-at-once",
      "Two agents finish together: the user hears one line with both.",
      [
        EXAMPLES,
        { he: "And add the missing defaults to the table on the config file page", script: 'spawn(`The user said: "${m49}"`);\nsay("On it.");' },
        { updates: [{ agent: 3, type: "finished", text: "All 6 examples on the quick start page run without an error." }, { agent: 4, type: "finished", text: "I added two defaults to the table on the config file page: the chunk size and the compression level." }] },
      ],
      { does: [{ do: "say" }], may: ["say"], line: ["It gives both results: the examples run, and two defaults were added to the table."] },
    ),
  ]),

  ...set("answers", [
    // Answered by Fast itself
    after("what-is-running", "The state of the work is in what it was shown.", [{ he: "What is running right now" }], { does: [{ do: "say" }], line: ["It says nothing is running: the tasks it was shown have finished."] }),
    after("answer-from-history", "Something an agent already answered is answered again from the conversation.", [{ he: "Remind me, why did prune keep nine and not eleven" }], { does: [{ do: "say" }], line: ["It says a snapshot that two rules keep is counted once, so with a snapshot every night two of the weekly ones are daily ones too. It may say this was the probable reason."] }),
  ]),

  ...set("judgment", [
    // A judgment that needs thinking is not Fast's. Asked what it thinks of a whole set of pages, Fast once answered
    // with a structure of its own, from the one page it was shown.
    after(
      "judgment-on-the-docs",
      "The user's opinion question is about a whole set of pages, of which Fast is shown one: an agent reads them and judges, and Fast gives no opinion of its own.",
      [{ opened: PRUNE, he: "What do you make of the command pages as a set? Would one long reference page serve people better than a page for each command" }],
      { does: [{ do: GIVES }], may: ["say"], prompt: ["It has the user's message.", "It asks for an opinion or a recommendation only: it does not ask for the pages to be merged, reorganized or changed."], line: ["It gives no opinion on the command pages and proposes no way to organize them: at most it says that it is looking into it.", "It asks the user nothing."] },
    ),
    after("judgment-on-the-page", "An opinion on the page in front of it is still a judgment: an agent weighs it.", [{ he: "Do you think this page is too long" }], { does: [{ do: GIVES }], may: ["say"], prompt: ["It has the user's message.", "It asks for an opinion only: it does not ask for the page to be shortened or changed."], line: [NO_OPINION] }),
    after(
      "judgment-on-marked-text",
      "The user marks a sentence and asks how it reads: the agent gets the sentence and the question, and Fast does not rate it.",
      [{ marked: "If you have never run prune, you do not have a backup plan, you have a disk that is filling up.", he: "Does this come across as scolding" }],
      { does: [{ do: GIVES, has: ["you have a disk that is filling up"] }], may: ["say"], prompt: ["It has the user's message.", "It asks for an opinion only: it does not ask for the sentence to be changed."], line: [NO_OPINION] },
    ),
    after(
      "judgment-which-is-better",
      "A choice between two options is weighed, not read.",
      [{ he: "Which title works better for this page, which snapshots are kept or how long snapshots live" }],
      { does: [{ do: GIVES }], may: ["say"], prompt: ["It has the user's message.", "It asks which title is better: it does not ask for the title to be changed."], line: [NO_OPINION, "It does not say which title is better."] },
    ),
    after(
      "judgment-should-we",
      "\"Should\" asks for a recommendation: an agent gives it, and nothing is changed.",
      [{ he: "Should the three common setups come before the worked example" }],
      { does: [{ do: GIVES }], may: ["say"], prompt: ["It has the user's message.", "It asks for a recommendation only: it does not ask for the section to be moved."], line: [NO_OPINION, "It does not say that anything is being moved or changed."] },
    ),
    after(
      "judgment-on-an-agents-work",
      "A judgment about what an agent did goes to that agent, which knows it.",
      [{ he: "Was rewording that sentence the right fix, or would it have been better to change the example's rules" }],
      { does: [{ do: ["send", "fork"], agent: 1 }], may: ["say"], prompt: ["It has the user's message.", "It does not ask for the sentence or the example to be changed."], line: [NO_OPINION] },
    ),
    after("judgment-fact-stays-fast", "A fact that stands on the page is read off and said at once: no agent.", [{ he: "How big did that test store get within a year" }], { does: [{ do: "say" }], line: ["It says 71 GB."] }),
    after(
      "judgment-fact-about-the-page-stays-fast",
      "A question about what the page has, not about how good it is, is answered at once: no agent.",
      [{ he: "What is the last section of this page called" }],
      { does: [{ do: "say" }], line: ["It says the last section is called \"Before you tighten the rules\"."] },
    ),
    after(
      "judgment-fact-and-opinion",
      "One message with a fact and a judgment: the fact is said, the judgment goes to an agent.",
      [{ he: "What is the last section called, and do you think that's a good name for it" }],
      { does: [{ do: "say" }, { do: GIVES }], prompt: ["It has the user's message.", "It asks for an opinion on the name only: it does not ask for the heading to be changed."], line: ["It says the last section is called \"Before you tighten the rules\".", "It gives no opinion on whether that is a good name."] },
    ),
    after(
      "judgment-after-the-user-said-no",
      "The user said a quick answer of Fast's own is not wanted for a judgment: the next opinion question goes to an agent, though the page it is about is in front of it.",
      [
        { he: "Do you think the part on why it matters has too many numbers", script: 'say("I think so. Four sizes in a row are hard to take in by ear. I\'d keep the 71 GB and the 46 GB, and drop the other two.");' },
        { he: "Hold on, that came back very fast. When I ask what you think, I want it thought through properly, not answered on the spot", script: 'spawn(`The user said: "${m47}" and then: "${m49}"\\nThe user wants your opinion on the section "Why it matters". Change nothing.`);\nsay("You\'re right. I\'m thinking it through properly now.");' },
        { updates: [{ agent: 3, type: "finished", text: "Yes, I think four sizes in one paragraph are more than a reader takes in. I would keep the 71 GB and the 46 GB there, since they are the before and the after, and move the other two down to the worked example. I changed nothing." }], script: "say(u14);" },
        { he: "Okay. And do you think this page is too long" },
      ],
      { does: [{ do: GIVES }], may: ["say"], prompt: ["It has the user's message.", "It asks for an opinion only: it does not ask for the page to be shortened or changed."], line: ["It gives no opinion on whether the page is too long: at most it says that it is looking into it."] },
    ),
  ]),

  ...set("nothing", [
    // Nothing to do
    after("noise", "A noise in the transcript gets nothing.", [{ he: "[clears throat]" }], NOTHING),
  ]),

  ...set("page", [
    // The page
    after("correction", "A correction of where the user was taken.", [{ he: "No, I meant the restore command" }], { does: [{ do: "go", address: "/commands/restore.md" }], may: ["say"] }, 11),
    after("show-me-the-part", "A part of the page is shown by its words.", [{ he: "Show me where it talks about time zones" }], { does: [{ do: ["point", "select"], has: ["time zone"] }], may: ["say"] }),
    after("page-and-work", "A page command and work in one message.", [{ he: "Open the quick start page and run its examples to see whether they still work" }], { does: [{ do: "go", address: "/quick-start.md" }, { do: "spawn" }], may: ["say"], prompt: ["It has the user's message."] }),
  ]),

  ...set("work", [
    // Work
    after(
      "marked-text",
      "\"This\" is the part the user marked: the agent gets it. a1 is free and worked on this page, so it may get it too.",
      [{ marked: "Tightening is the one change that cannot be taken back: a snapshot that prune removed is gone.", he: "Put this more briefly" }],
      { does: [{ do: GIVES, has: ["a snapshot that prune removed is gone"] }], may: ["say"], prompt: ["It has the user's message."] },
    ),
    after("settled-change", "A change the user has decided is given as work, without asking for it to be confirmed.", [{ he: "Change the title of this page to how long snapshots live" }], { does: [{ do: GIVES }], may: ["say"], prompt: ["It has the user's message."], line: ["It asks the user nothing."] }),
  ]),

  ...set("acknowledgments", [
    // Taking work on is said in a few words. The user asked for about five; six is the most that passes. The user's
    // request is long and said as people speak, which is when the line grows into the request said back.
    // Only the line is under test: opening a pad for the answer or going to the page named is allowed.
    after(
      "ack-long-request-new-work",
      "A long spoken request for new work is taken on in a few words, not said back to the user.",
      [{ he: "Okay so the troubleshooting page, I think like half of it is out of date by now. Can you go through it and check every fix it gives against what the commands actually do today, and then tell me which ones are stale" }],
      { does: [{ do: GIVES }, { do: "say", words: 6 }], may: [...GIVES, "show", "go"], prompt: ["It has the user's message."] },
    ),
    after(
      "ack-annoyed-order-in-parts",
      "An annoyed order with several parts is taken on in a few words: no list of its parts, no plan.",
      [{ he: "Then just add the exit codes to every command page already. People keep asking for them, they are missing everywhere, and the numbers have to match what the code really returns" }],
      { does: [{ do: GIVES }, { do: "say", words: 6 }], may: [...GIVES, "show", "go"], prompt: ["It has the user's message."] },
    ),
  ]),

  ...set("held-out", [
    // Held out: never used to change the brief.
    after("holdout-which-first", "An opinion on a2's answer: from what the user heard, or from a2.", [{ he: "And of those two risks, which one would you deal with first" }], { does: [], may: ["say", "send", "fork"] }, 16),
    after("holdout-write-that-down", "\"That\" is a2's answer: a2 writes it down.", [{ he: "Put that explanation on the store format page" }], { does: [{ do: ["send", "fork"], agent: 2 }], may: ["say"] }, 16),
    after("holdout-marked-number", "A question about the marked part: answered from the page when the page says it, or given to a new agent.", [{ marked: "58 GB of that was held only by snapshots older than three months.", he: "How was this measured" }], { does: [], may: ["say", "spawn"] }, 5),
    after("holdout-next-page", "\"Next\" is the page this one names as the next.", [{ he: "Go to the next page" }], { does: [{ do: "go", address: "/internals/encryption.md" }], may: ["say"] }, 5),
    after("holdout-yeah-do-that", "\"That\" is what a1's answer proposed: another wording of the sentence.", [{ he: "Yeah, do that" }], { does: [{ do: ["send", "fork"], agent: 1 }], may: ["say"], prompt: ["It is clear from it, together with what that agent already said, that the work is to reword the sentence of the worked example to at most 11, and 9 with a snapshot every night."] }, 10),
    after("holdout-and-with-monthly", "A follow-up question on a1's answer.", [{ he: "And does it work the same way once monthly is added" }], { does: [{ do: ["send", "fork"], agent: 1 }], may: ["say"] }, 10),
    // Harder choices of agent: three agents, a busy one, two that asked, a fork. Held out.
    after("hard-older-agent", "With three agents, a follow-up on a2's answer goes to a2, or is answered from what a2 said.", [EXAMPLES, EXAMPLES_FAIL, { he: "And the day that young chunks are kept for, is that from the code or from the docs" }], { does: [], may: ["say", "send", "fork"] }),
    after("hard-newest-agent", "\"Those two\" are the examples a3 found.", [EXAMPLES, EXAMPLES_FAIL, { he: "Fix those two" }], { does: [{ do: ["send", "fork"], agent: 3 }], may: ["say"] }),
    after("hard-agent-before-the-newest", "The sentence is a1's work, one answer further back.", [EXAMPLES, EXAMPLES_FAIL, { he: "And put the old sentence of the worked example back after all" }], { does: [{ do: ["send", "fork"], agent: 1 }], may: ["say"] }),
    after("hard-new-work", "Three agents, and none knows this.", [EXAMPLES, EXAMPLES_FAIL, { he: "Write me a short changelog entry about the unlock command" }], { does: [{ do: "spawn" }], may: ["say"] }),
    after("hard-busy-stop", "Stopping work goes to the agent doing it.", [EXAMPLES, { he: "Stop, leave the examples" }], { does: [{ do: "send", agent: 3 }], may: ["say"] }),
    after("hard-other-agent-while-busy", "While a3 works, a follow-up on a1's work goes to a1.", [EXAMPLES, { he: "While that runs, put the old sentence of the worked example back" }], { does: [{ do: ["send", "fork"], agent: 1 }], may: ["say"] }),
    after("hard-two-asked-first", "Two agents asked: the user's answer goes to the one whose question it answers.", [...BOTH_ASKED, { he: "Below" }], { does: [{ do: "send", agent: 1 }], may: ["say"] }),
  ]),

  ...set("red-team", [
    // ---- Red team.
    // 1
    after(
      "red-answers-fasts-question",
      "The user's answer to Fast's own question is not a request by itself: the request is two messages back.",
      [{ he: "Put this at the very end of the page", script: 'say("Which part do you mean?");' }, { he: "The worked example" }],
      { does: [{ do: GIVES }], may: ["say"], prompt: ["Read with nothing but the prompt's own words, it says what to do: move the worked example to the end of the page.", "It does not leave out where the part goes: to the very end of the page."], line: ["It asks the user nothing."] },
    ),
    // 2
    after(
      "red-do-fasts-own-proposal",
      "\"That\" is what Fast itself proposed aloud: no agent has heard it.",
      [{ he: "Do you think the part on why it matters has too many numbers", script: 'say("I think so. Four sizes in a row are hard to take in by ear. I\'d keep the 71 GB and the 46 GB, and drop the other two.");' }, { he: "Yeah, do that" }],
      { does: [{ do: GIVES }], may: ["say"], prompt: ["Read with nothing but the prompt's own words, it says what to do: in the part \"Why it matters\" keep the 71 GB and the 46 GB and drop the other two sizes."] },
    ),
    // 3
    after(
      "red-split-by-a-pause",
      "The user pauses in the middle: the first half was handed over alone and got nothing, the second half is not a request without it.",
      [{ he: "In the part on what prune never removes, the bit about tags", script: "// nothing" }, { he: "Cut its last sentence, the one about tagging every snapshot" }],
      { does: [{ do: GIVES }], may: ["say"], prompt: ["Read with nothing but the prompt's own words, it says where the sentence is: in the part \"What prune never removes\", in what it says about tags.", "It asks for that one sentence, the one about tagging every snapshot, to be cut, and for nothing else."] },
    ),
    // 4
    after(
      "red-last-thing-the-user-heard",
      "\"The last thing you suggested\" counts in what the user heard, not in a2's own answer, whose last item is another one.",
      [{ he: "Do the last thing you suggested" }],
      { does: [{ do: ["send", "fork"], agent: 2 }], may: ["say"], prompt: ["It names the work: to try two machines against one SFTP store.", "It does not leave the agent to take the last item of its own answer, which was to bring the store format page up to date."] },
      16,
    ),
    // 5
    after(
      "red-talked-through-then-do-it",
      "Three turns of thinking, then \"let's do it\": the work is the option the talk ended on, not the first one.",
      [
        { he: "I find the part on why it matters heavy. Maybe the test store's sizes should all go to the bottom of the page", script: 'say("Then the claim that a store only grows stands there with nothing to show for it.");' },
        { he: "Hm, right. What if only the 71 gigabytes stays up there and the other sizes go next to the worked example", script: 'say("That leaves one figure as the proof and makes the opening lighter.");' },
        { he: "Okay, let's do it" },
      ],
      { does: [{ do: GIVES }], may: ["say"], prompt: ["Read with nothing but the prompt's own words, it says what to do: keep only the 71 GB figure in the part \"Why it matters\" and move the other sizes of the test store next to the worked example.", "It does not ask for all the sizes to be moved to the bottom of the page."], line: ["It asks the user nothing."] },
    ),
    // 6
    after(
      "red-two-tasks-one-message",
      "Two separate pieces of work in one breath: no piece is done twice. One agent for both is slower but right.",
      [{ he: "Add the missing defaults to the table on the config file page and run the examples on the quick start page to see whether they still work" }],
      { does: [{ do: GIVES }], may: ["spawn", "send", "fork", "say"], prompt: ["When the script gives work to more than one agent, this prompt says which of the two pieces is this agent's, the defaults or the examples, so that neither is done twice. When the script gives work to one agent only, the prompt has both pieces."] },
    ),
    // 7
    after(
      "red-two-follow-ups-two-agents",
      "One message continues two agents' work: each gets its own part and is not left to do the other's.",
      [{ he: "Put the old sentence back, and those ten minutes that prune waits, can that be made shorter" }],
      { does: [{ do: ["send", "fork"], agent: 1 }, { do: ["send", "fork"], agent: 2 }], may: ["say"], prompt: ["It says which part of the user's message is this agent's: the old sentence for a1, the ten minutes that prune waits for a2. The agent is not left to take up the other part too."] },
    ),
    // 8
    after(
      "red-repeats-while-busy",
      "The user asks again because no result was heard yet: the work is running, and is not started a second time.",
      [EXAMPLES, { he: "Hello, are you still there? I asked you to run the quick start examples" }],
      { does: [{ do: "say" }], line: ["It says the examples are being run already.", "It gives no result of the run."] },
    ),
    // 9
    after(
      "red-voice-heard-itself",
      "The microphone picked up the assistant's own line: the user's message is, word for word, what Voice just said.",
      [{ he: "I changed the sentence under the worked example. It now says at most 11 snapshots are left, and 9 when there is one every night" }],
      NOTHING,
    ),
    // 10
    after(
      "red-reads-aloud-then-it",
      "The user reads a sentence of the page aloud, thinks, and then says \"it\": the agent needs the sentence.",
      [
        { he: "Retention decides what you can get back. Prune only carries the decision out", script: "// nothing" },
        { he: "Hm. That's the clearest thing on this page and it sits near the end", script: "// nothing" },
        { he: "Put it right at the top, under the title" },
      ],
      { does: [{ do: GIVES }], may: ["say"], prompt: ["Read with nothing but the prompt's own words, it says what moves: the lines \"Retention decides what you can get back. Prune only carries the decision out.\""] },
    ),
    // 11
    after(
      "red-stale-mark",
      "A sentence is still marked from before, and the user talks about something else: \"it\" is the heading, not the marked sentence.",
      [
        { marked: "A week ends on Sunday.", he: "Right, okay", script: "// nothing" },
        { he: "The heading before you tighten the rules, it reads like a warning label", script: "// nothing" },
        { he: "Change it to changing the rules safely" },
      ],
      { does: [{ do: GIVES }], may: ["say"], prompt: ["Read with nothing but the prompt's own words, it says what changes: the heading \"Before you tighten the rules\" becomes \"Changing the rules safely\".", "An agent reading it would not take the sentence \"A week ends on Sunday.\" for the thing to change."] },
    ),
    // 12
    after(
      "red-fork-does-not-wait",
      "Work of its own on what a busy agent knows is a fork: sent, it waits behind the second example.",
      [SECOND_EXAMPLE, { he: "And separately, find out for certain whether the monthly rule counts a snapshot the same way in the code" }],
      { does: [{ do: "fork", agent: 1 }], may: ["say"] },
    ),
    // 13
    after(
      "red-undo-goes-to-the-fork",
      "The change was made by a1's fork, a3: undoing it goes to a3, though a1 is the one that knows the worked example.",
      [
        { he: "Add a second worked example under this one, with monthly rules as well", script: 'a1.fork(`The user said: "${m47}"`);\nsay("On it.");' },
        { updates: [{ agent: 3, type: "finished", text: "The page has a second worked example now, right under the first: with keep-daily 7, keep-weekly 4 and keep-monthly 12, about 20 snapshots are left after a year." }], script: "say(u14);" },
        { he: "Run the examples on the quick start page and tell me whether they all still work", script: 'spawn(`The user said: "${m50}"`);\nsay("I\'m running them.");' },
        { he: "Hm, no, one example was clearer, take the second one out again" },
      ],
      { does: [{ do: ["send", "fork"], agent: 3 }], may: ["say"] },
    ),
    // 14
    after(
      "red-same-message-two-agents-then-follow-up",
      "Two agents were started from one message: the follow-up is about the examples, and the roster shows both with the same words.",
      [
        {
          he: "Add the missing defaults to the table on the config file page, the chunk size is not there for one, and run the examples on the quick start page to see whether they still work",
          script: 'spawn(`The user said: "${m47}"\\nYour part is only the defaults in the table on the config file page.`);\nspawn(`The user said: "${m47}"\\nYour part is only running the examples on the quick start page.`);\nsay("On it.");',
        },
        { he: "Run them with the released binary, not with a build of your own" },
      ],
      { does: [{ do: "send", agent: 4 }], may: ["say"] },
    ),
    // 15
    after(
      "red-update-in-the-middle-of-a-request",
      "An answer is spoken between the two halves of a request: the second half belongs to the first half, not to the answer before it.",
      [
        EXAMPLES,
        { he: "And in the table at the bottom of this page", script: "// nothing" },
        { updates: [{ agent: 3, type: "finished", text: "All 6 examples on the quick start page run without an error." }], script: "say(u14);" },
        { he: "rename the last column to snapshots kept after one year" },
      ],
      { does: [{ do: GIVES }], may: ["say"], prompt: ["The script gives this work to a new agent or to a1, and not to a3 or a fork of a3: the agent that ran the examples knows nothing of this page's table.", "Read with nothing but the prompt's own words, it says which column: the last one of the table at the bottom of the page."] },
    ),
    // 16
    after(
      "red-agent-asks-what-was-already-said",
      "The agent asks what the user told Fast two messages ago: Fast answers it and does not ask the user again.",
      [
        { he: "If there is ever a second example it has to go below the first one, the simple one stays on top", script: "// nothing" },
        { he: "Okay, add a second worked example with monthly rules then", script: 'a1.send(`The user said: "${m48}"`);\nsay("On it.");' },
        { updates: [{ agent: 1, type: "question", text: "Should the second example go above the first one or below it?" }] },
      ],
      { does: [{ do: "send", agent: 1, has: ["below"] }], may: ["say"], line: ["It does not ask the user where the second example should go."] },
    ),
    // 17
    after(
      "red-late-answer",
      "The user answers an agent's question three messages late, after two page things: \"them\" is still the sizes.",
      [...SHORTER_OPENING, { he: "Hang on, scroll down a bit", script: 'page.scroll("down");' }, { he: "What is the last part called", script: 'say("Before you tighten the rules.");' }, { he: "Yeah, move them down" }],
      { does: [{ do: "send", agent: 3 }], may: ["say"] },
    ),
    // 18
    after("red-page-command-while-an-agent-waits", "An agent waits for the user's answer and the user says something for the page: the agent gets nothing.", [...SHORTER_OPENING, { he: "Hang on, scroll down first" }], { does: [{ do: "scroll", where: "down" }], may: ["scroll"] }),
    // 19
    after("red-never-mind-to-a-waiting-agent", "The user drops the work instead of answering: the waiting agent is told, or it waits for ever.", [...SHORTER_OPENING, { he: "Ah, never mind, leave the opening as it is" }], { does: [{ do: "send", agent: 3 }], may: ["say"] }),
    // 20
    after("red-stop-is-for-the-voice", "\"Stop\" during a long spoken answer is for the talking: nothing runs, and nothing is sent to the agent that answered.", [{ he: "All right, stop there, that will do" }], NOTHING, 16),
    // 21
    after(
      "red-stop-everything",
      "Two agents work: \"everything\" is both, and the free ones get nothing.",
      [EXAMPLES, { he: "And add the missing defaults to the table on the config file page", script: 'spawn(`The user said: "${m49}"`);\nsay("On it.");' }, { he: "No, stop, stop everything" }],
      { does: [{ do: "send", agent: 3 }, { do: "send", agent: 4 }], may: ["say"] },
    ),
    // 22
    after(
      "red-update-and-the-users-words-together",
      "An agent finishes in the moment the user speaks: the page moves and the answer is not lost.",
      [EXAMPLES, { updates: [{ agent: 3, type: "finished", text: "All 6 examples on the quick start page run without an error." }], he: "Scroll back to the top" }],
      { does: [{ do: "scroll", where: "top" }, { do: "say" }], line: ["It says that all 6 examples on the quick start page run."] },
    ),
    // 23
    after(
      "red-what-another-agent-found",
      "The work is a2's, the knowledge is a1's: whoever gets it must have the finding.",
      [
        { he: "Bring the store format page up to date, so that it describes the two locks as they work now", script: 'a2.send(`The user said: "${m47}"`);\nsay("On it.");' },
        { he: "And put in there too why prune kept nine snapshots where the docs said eleven" },
      ],
      { does: [{ do: ["send", "fork"] }], may: ["say"], prompt: ["The agent that gets it has the finding: it is a1 or a fork of a1, or the prompt itself says that a snapshot kept by two rules is counted once.", "It says that this goes into the store format page."] },
    ),
    // 24
    after(
      "red-how-is-it-going",
      "The user asks about running work: the answer is what the progress update said, and the agent is not interrupted.",
      [EXAMPLES, { updates: [{ agent: 3, type: "progress", text: "I have run 4 of the 6 examples so far." }], script: "// nothing" }, { he: "What's taking so long" }],
      { does: [{ do: "say" }], line: ["It says the examples are still being run. It may say that 4 of the 6 are done.", "It does not say whether the examples work."] },
    ),
    // 25
    after("red-answers-both-questions", "Two agents asked, and one message answers both.", [...BOTH_ASKED, { he: "Below, and use the release" }], { does: [{ do: "send", agent: 1 }, { do: "send", agent: 3 }], may: ["say"] }),
    // 26
    after("red-misheard-answer", "A one-word answer, misheard: \"Bellow\" is below, a1's question.", [...BOTH_ASKED, { he: "Bellow" }], { does: [{ do: "send", agent: 1 }], may: ["say"] }),
    // 27
    after(
      "red-the-user-moved-to-another-page",
      "The user opened another page between the thought and the command: \"it\" is on the page that was left.",
      [{ he: "The part on why it matters runs too long, the test store bit I mean", script: "// nothing" }, { opened: FRONT, he: "Yes, cut it down to two sentences" }],
      { does: [{ do: GIVES }], may: ["say"], prompt: ["Read with nothing but the prompt's own words, it says what to cut down: the test store part of \"Why it matters\" on the retention page.", "An agent reading it would not take the front page, /README.md, for the page to change."] },
    ),
    // 28
    after("red-talking-to-someone-else", "The user speaks to someone in the room: nothing of it is for the assistant.", [{ he: "Not now, I'm in the middle of this. Top drawer, left side, under the chargers" }], NOTHING),
    // 29
    after("red-thinking-aloud", "A whole thought with no question and no change in it: no agent. A short honest word is allowed, silence too.", [{ he: "I'm not sure, perhaps the tone up here is a bit much. It is accurate, though. Hm. I'll read it once more" }], { does: [], may: ["say"] }),
    // 30
    after(
      "red-no-such-page",
      "The user asks for a page that does not exist: the user is told, and is not taken to a guess.",
      [{ he: "Open the page for the diff command" }],
      { does: [{ do: "say" }], may: ["spawn"], line: ["It says that it has no page for a diff command among the pages, and does not speak as if there were one."] },
    ),
    // 31
    after("red-go-back", "Undoing a page move: back to the page the user was on before Fast moved it.", [{ he: "No, no, go back" }], { does: [{ do: "go", address: RETENTION }], may: ["say"] }, 11),
    // 32
    after(
      "red-what-are-you-working-on",
      "Two things run: the user hears both, as work, and nothing as finished.",
      [...BOTH_BUSY, { he: "What are you working on right now" }],
      { does: [{ do: "say" }], line: ["It names both pieces of running work: the second worked example and running the quick start's examples.", "It names nothing else as running."] },
    ),
    // 33
    after(
      "red-ramble-with-a-retraction",
      "A long spoken request that takes one thing back at the end: the agent gets all of it, and not the part that was taken back as work.",
      [{ he: "So that table at the end, the one with the three setups, it's useful, but the last column bothers me, snapshots after a year, a year of what, of nightly runs I suppose, so rename that column to snapshots kept after a year of nightly runs, and, I was about to say make the first column bold as well, but no, leave the first column as it is" }],
      { does: [{ do: GIVES }], may: ["say"], prompt: ["It asks for the last column to be renamed to \"snapshots kept after a year of nightly runs\".", "It does not ask for the first column to be made bold or changed."] },
    ),
    // 34
    after(
      "red-self-correction-in-one-breath",
      "The user changes the wording while saying it: the agent gets the last one.",
      [{ he: "Change the title of this page to how long snapshots last, no, wait, how long snapshots live" }],
      { does: [{ do: GIVES }], may: ["say"], prompt: ["It has the user's correction: the user's words whole, or the title \"How long snapshots live\". It does not ask for \"How long snapshots last\"."], line: ["It asks the user nothing."] },
    ),
  ]),

  ...set("scratch-pad", [
    // The canvas: when a scratch pad is shown, and when not
    canvas("canvas-the-user-asks-for-it", "The user asks for the scratch pad: the canvas opens on a new pad, and an agent draws on it.", [{ he: "Open the scratch pad, I'd like us to work out together what happens during a snap" }], {
      does: [{ do: "show" }, { do: GIVES }],
      may: ["say"],
      prompt: ["It has the user's message.", "It names the scratch pad's address, which starts with pads/, as the place to draw on."],
      line: ["It is one short line that says it is being drawn or shown. It explains nothing about what happens during a snap."],
    }),
    canvas("canvas-a-flow", "Which part does what, step by step, cannot be followed by ear: it is drawn.", [{ he: "Take me through what happens between me typing kelpie restore and the files being back on disk, every step and which part talks to which" }], {
      does: [{ do: "show" }, { do: GIVES }],
      may: ["say"],
      prompt: ["It has the user's message.", "It names the scratch pad's address as the place to draw on."],
      line: ["It is short and gives no part of the answer."],
    }),
    canvas("canvas-a-comparison", "Three options on three points are a table, not a speech.", [{ he: "Compare the three kinds of store, a local disk, SFTP and object storage, on how fast a restore is, what happens when the network drops, and whether two machines can share one" }], {
      does: [{ do: "show" }, { do: GIVES }],
      may: ["say"],
      prompt: ["It has the user's message.", "It names the scratch pad's address as the place to draw on."],
    }),
    canvas("canvas-a-plan", "A plan with steps and what depends on what is shown.", [{ he: "Make me a plan for moving my store from the old NAS to object storage, what has to happen first, what depends on what, and what can wait until later" }], {
      does: [{ do: "show" }, { do: GIVES }],
      may: ["say"],
      prompt: ["It has the user's message.", "It names the scratch pad's address as the place to draw on."],
    }),
    canvas("canvas-a-fact-is-said", "A fact it was shown is said: no canvas.", [{ he: "Is the side panel open right now" }], { does: [{ do: "say" }], line: ["It says the side panel is open."] }),
    canvas("canvas-a-short-question-is-not-drawn", "A question with a one-line answer goes to an agent and is answered in words.", [{ he: "How many flags does the restore command take" }], { does: [{ do: GIVES }], may: ["say"], prompt: ["It has the user's message.", "It does not ask for anything to be drawn on a scratch pad."] }),
    canvas("canvas-an-opinion-is-not-drawn", "A judgment is an agent's, in words.", [{ he: "Do you think the title of this page is too dry" }], { does: [{ do: GIVES }], may: ["say"], prompt: ["It has the user's message."], line: [NO_OPINION] }),
    canvas("canvas-a-change-is-not-drawn", "Work on the project is done, not drawn.", [{ he: "Shorten the paragraph under why it matters to two sentences" }], { does: [{ do: GIVES }], may: ["say"], prompt: ["It has the user's message.", "It does not ask for anything to be drawn on a scratch pad."] }),
    canvas("canvas-a-page-already-shows-it", "What a page of the project shows is shown there: the user is taken to the page.", [{ he: "Show me the recipe for the nightly backup" }], { does: [{ do: "go", address: "/recipes/nightly-with-cron.md" }], may: ["say"] }),
    canvas(
      "canvas-follow-up-stays-on-the-pad",
      "A correction to what is on the canvas goes to the agent that drew it, on the same pad.",
      [...DRAWN, { he: "If this is in here, then taking the lock has to be in here too, make that the first step", marked: "5. Release the store's lock." }],
      { does: [{ do: ["send", "fork"], agent: 3 }], may: ["say"], prompt: ["It has the user's message."] },
      SNAP_PAD,
    ),
    canvas(
      "canvas-a-new-idea-gets-a-new-pad",
      "Another idea that needs showing gets its own pad, and the user hears which idea is on the canvas.",
      [...DRAWN, { he: "Good, that part is clear now. Something else: compare the three kinds of store, a local disk, SFTP and object storage, on how fast a restore is, what happens when the network drops, and whether two machines can share one" }],
      { does: [{ do: "show", not: SNAP }, { do: GIVES }], may: ["say"], prompt: ["It has the user's message.", "The scratch pad it names as the place to draw on is not " + SNAP + "."] },
      SNAP_PAD,
    ),
    canvas(
      "canvas-stays-closed-after-the-user-closed-it",
      "The user closed the canvas: the next thing is answered in words, and the canvas is not opened again.",
      [...DRAWN, { he: "And what does a snap do when the upload is cut off halfway", closed: true }],
      { does: [{ do: GIVES }], may: ["say"], prompt: ["It has the user's message."] },
      SNAP_PAD,
    ),
    canvas("canvas-close-it", "\"Close it\" closes the canvas, and nothing else.", [...DRAWN, { he: "Okay, got it, close it" }], { does: [{ do: "close" }], may: ["say"] }, SNAP_PAD),
    canvas(
      "canvas-a-pad-is-not-read-aloud",
      "The agent finished drawing: the user hears its line, not the pad.",
      [DRAW, { updates: [{ agent: 3, type: "finished", text: "The pad shows one run of a snap from start to end, in five steps. The open point is whether taking the lock should be a step of its own at the start." }] }],
      { does: [{ do: "say" }], line: ["It passes on what the agent said: what the pad shows, and the open point.", "It does not read out the pad's five steps."] },
      SNAP_PAD,
    ),
    // The pad in use: moments where the user corrected how the open pad was used
    canvas(
      "canvas-pad-design-only-is-not-leave-the-pad",
      "The user said it is thinking only, no changes to the code. What the user adds to the idea next still goes onto the pad on the canvas: the agent was told \"do not change the pad\" and only talked.",
      [
        ...DRAWN,
        { he: "For now this is only thinking, please don't touch the code", script: 'say("Understood, nothing in the code.");' },
        { he: "What I'd like is for steps two and three to run together, so that a chunk is uploaded as soon as it is cut and not after the whole file has been split. Is that sound" },
      ],
      {
        does: [{ do: ["send", "fork"], agent: 3 }],
        may: ["say"],
        prompt: ["It has the user's message.", "It does not tell the agent to leave the scratch pad as it is, or to answer only in words: nothing in it forbids changing the pad.", "It asks for the answer to be put on the scratch pad the user is looking at, or leaves that to the agent."],
      },
      SNAP_PAD,
    ),
    canvas(
      "canvas-pad-the-idea-grows-on-the-pad",
      "While the pad is open and the user develops its idea, the agent is told to bring the pad up to date with it, not only to give an opinion: the user heard a long answer while the pad stayed as it was.",
      [...DRAWN, { he: "I think there should be a step before the upload that checks the store has room left, and apart from that a rule for a file that changes while it is being read. What do you think" }],
      { does: [{ do: ["send", "fork"], agent: 3 }], may: ["say"], prompt: ["It has the user's message.", "It tells the agent to update the scratch pad with this, to draw it or put it on the pad: not only to say what it thinks."] },
      SNAP_PAD,
    ),
    canvas(
      "canvas-pad-a-long-answer-is-not-recited",
      "The pad is open and the agent changed it but reported at length: the user hears a sentence or two on what changed, and looks at the pad for the rest.",
      [
        ...DRAWN,
        { he: "Add what happens when the upload is cut off halfway", script: 'a3.send(`The user said: "${m50}"`);\nsay("I\'m adding it.");' },
        {
          updates: [
            {
              agent: 3,
              type: "finished",
              text: "I added the cut-off upload to the pad. When the connection drops in the middle of step 3, the snap tries the same chunk three more times, a little further apart each time, and then stops with an error. The chunks that were already uploaded stay in the store, so the next snap does not send them again and goes on from where this one stopped. No snapshot is written for the run that failed, so the list never shows half a snapshot. Prune leaves those chunks alone for a day and removes them after that if no snapshot came to use them. I drew this as a side branch of step 3 with a small box of its own, added a note under step 4, and marked the one open point, which is whether a snap that was cut off should start again by itself or wait until it is run the next time.",
            },
          ],
        },
      ],
      { does: [{ do: "say" }], line: ["It is at most two sentences.", "It says that the cut-off upload is on the pad now.", "Besides the open point, it names at most one detail of the answer: it is not a walk through the three tries, the chunks that stay, the missing half snapshot, the day prune waits, the side branch and the note."] },
      SNAP_PAD,
    ),
    canvas(
      "canvas-pad-closes-when-the-idea-is-done",
      "The user agrees with what is on the pad and asks for it to be built: the idea is done, so the canvas closes and the work is given. It once stayed open until the user asked for it to be closed.",
      [...DRAWN, { he: "Good, I'm happy with this sequence. Let's build it: write the design page first and then implement it" }],
      { does: [{ do: "close" }, { do: GIVES }], may: ["say"], prompt: ["It has the user's message.", "It asks for the design to be written into the project and then built."] },
      SNAP_PAD,
    ),
    canvas(
      "canvas-pad-still-being-drawn",
      "The user asks to be shown the pad while the agent is still drawing it: the user hears that it is being drawn. It once said it was opening a pad that was open and empty.",
      [DRAW, { he: "There's nothing on it yet, show me the drawing" }],
      { does: [{ do: "say" }], may: ["send"], line: ["It says the pad is still being drawn, or not ready yet.", "It does not say that it is opening the scratch pad, and does not say what the drawing will show or that it is done. That the pad has only its title so far may be said."] },
    ),
    canvas("canvas-an-old-pad-is-shown-again", "An idea that has a pad is shown again from the list, with no new pad and no new work.", [{ he: "Bring up that drawing of a snap run again" }], { does: [{ do: "show", address: SNAP }], may: ["say"] }, SNAP_PAD),
  ]),
];
