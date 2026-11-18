// Voice: one GPT-Live session, which listens and talks, and nothing else. It runs in client mode: what the user says and what
// it says arrive as transcript fragments, and when it hands the user's words over the service gets only an event that says so, with a
// time. This file turns the fragments into messages, pairs each hand-over with the words it is about, and gives Voice
// the lines to say. Deciding what to do with a hand-over is Fast's part.
import { SHARED_BRIEF } from "./brief.ts";
import { shape } from "./log.ts";
import type { Provider } from "./provider.ts";

/** The voice model: `DEVPAGE_LIVE_MODEL` in the environment names another, and on Azure the deployment. */
const MODEL = process.env.DEVPAGE_LIVE_MODEL || "gpt-live-1";
/** How long a transcript may stay quiet before what came so far is one message. Voice hands the user's words over about two seconds after the user stops, so the user's wait longer than that. */
const PAUSE = { me: 3500, voice: 2000 };
/** After a hand-over, how long the user's transcript must stay quiet before its words are taken as complete, and the longest that is waited for. */
const SETTLE = 80;
const SETTLE_MOST = 1500;
/** The most one line may have: the API takes 500 tokens per append. */
const LINE = 1600;

/** A piece of a transcript, as the API sent it. The times are on the session's own clock. */
interface Fragment {
  event_id: string;
  delta: string;
  start_ms: number | null;
  end_ms: number | null;
}

/** What the user or Voice said, put together from its fragments. */
export interface Said {
  who: "me" | "voice";
  text: string;
  start_ms: number | null;
  end_ms: number | null;
  /** The hand-over these words came with, when Voice handed them over. */
  handover: string | null;
}

export interface VoiceEvents {
  /** The voice model's speech: mono 16-bit PCM at 24 kHz. */
  audio: (pcm: Uint8Array) => void;
  /** A piece of what was heard or said, as it arrives: for the page. */
  heard: (who: "me" | "ai", text: string) => void;
  /** The user or Voice said something: a complete message. When it has a hand-over, Fast is to be woken for it. */
  said: (message: Said) => void;
  /** A hand-over came after its words had already become messages. False when no message of the user's was waiting for one. */
  handed: (handover: string) => boolean;
  /** The session ended: the user stopped it, or why it stopped by itself. */
  closed: (why: string | null) => void;
  /** Something for the session's log. */
  log: (who: "user" | "voice" | "say" | "service", kind: string, fields: { text?: string; [more: string]: unknown }) => void;
}

const VOICE_BRIEF = `${SHARED_BRIEF}

You are Voice: the interface. You hear the user, and you speak to the user. You know nothing about the project, the page or the work, and you cannot see or change anything yourself. Everything the user says is answered, and everything is done, by the work you start when you generate <|SpawnThinking|>.

Generate <|SpawnThinking|> every time the user stops speaking, at once, for everything the user said since the last time:
- a question or a command;
- a remark, an opinion, an explanation, or a description of something the user sees or of a problem;
- a complaint, also one about you;
- thinking aloud, half a sentence, or something the user adds to what was said before;
- anything the user says while earlier work is still running.
You never decide that something the user said is not worth it, is not meant for you, or needs no answer: that is not yours to judge. Do not wait for the user to finish a thought, to ask a question or to tell you to act, and do not ask what the user means.

Do not generate <|SpawnThinking|> only when the user said no words: a sigh, a cough, a yawn, a laugh, a noise, or one filler word such as "um" by itself.

What you say is only what you are given to say. Each line you are given, say naturally, as a person talking, and completely: every sentence of it, in its order, with its meaning unchanged. Do not shorten it, summarise it, add to it, comment on it or soften it.

A line can arrive at any time, also long after the user spoke: it is news about work the user asked for earlier. Never talk over the user. When a line arrives while the user is speaking, wait until the user has stopped, and say it then.

When nothing is given to you to say, say nothing: not every thing the user says gets a reply. Never answer from what you know yourself, never guess, never acknowledge with words of your own, never repeat or rephrase what the user said, and promise nothing.`;

export class Voice {
  private readonly socket: WebSocket;
  private readonly events: VoiceEvents;
  /** True once the user stopped the session themselves. */
  stopped = false;
  private sent = 0;
  /** The fragments that are not a message yet. */
  private readonly growing = {
    me: { fragments: [] as Fragment[], timer: null as ReturnType<typeof setTimeout> | null },
    voice: { fragments: [] as Fragment[], timer: null as ReturnType<typeof setTimeout> | null },
  };
  /** The hand-over whose words are still being collected. */
  private handover: { id: string; offset_ms: number | null; at: number; timer: ReturnType<typeof setTimeout> | null } | null = null;
  /** A hand-over that came with no words: the next thing the user says is taken as its words. */
  private wordless: string | null = null;

  /** Resolves once the session has started; rejects with the reason when it doesn't. */
  readonly started: Promise<void>;

  constructor(provider: Provider, events: VoiceEvents) {
    this.events = events;
    // Bun's WebSocket takes headers; the browser's does not, and the key never goes there.
    const asked = Date.now();
    let connected = asked;
    this.socket = new WebSocket(provider.live, { headers: provider.headers } as any);
    this.started = new Promise((started, failed) => {
      let open = false;
      this.socket.onopen = () => {
        connected = Date.now();
        this.events.log("service", "voice_connected", { connect_ms: connected - asked });
        this.send({
          type: "session.start",
          session: {
            model: MODEL,
            instructions: VOICE_BRIEF,
            // The automatic "one moment" is asked for only after this long, and every hand-over cancels it: what the user
            // hears is only what Fast gives. 800 is the most the API accepts.
            delegation: { type: "client", acknowledgement_delay_ms: 800 },
          },
        });
      };
      this.socket.onmessage = (message) => {
        const event = JSON.parse(String(message.data));
        if (event.type === "session.started") {
          open = true;
          this.events.log("service", "voice_session_started", { model: MODEL, connect_ms: connected - asked, session_ms: Date.now() - connected, event: shape(event) });
          started();
        } else if (event.type === "error" && !open) {
          failed(new Error(event.error?.message ?? JSON.stringify(event.error)));
          this.socket.close();
        } else {
          this.on(event);
        }
      };
      this.socket.onclose = (close) => {
        // A session that never started ends in `started` rejecting, and in nothing else.
        if (!open) return failed(new Error(close.reason || "GPT-Live closed the connection before the session started"));
        if (this.handover?.timer) clearTimeout(this.handover.timer);
        this.message("me", this.handover?.id ?? null);
        this.message("voice", null);
        this.events.closed(this.stopped ? null : close.reason || "GPT-Live closed the session");
      };
    });
  }

  /** The user's voice: mono 16-bit PCM at 24 kHz. */
  hear(pcm: Uint8Array) {
    this.send({ type: "session.input_audio.append", audio: Buffer.from(pcm).toString("base64") });
  }

  /**
   * A line for Voice to say. `handover` is the hand-over it answers, or the one that started the work it is news of;
   * null when there is none. False when the connection was not open to take it.
   */
  say(text: string, handover: string | null, why: Record<string, unknown> = {}) {
    const content = text.length > LINE ? `${text.slice(0, LINE)}…` : text;
    const event_id = `say_${this.sent}`;
    const sent = this.send({ type: "session.commentary.append", event_id, delegation_id: handover, content });
    this.events.log("say", "line", { text: content, sent, event_id, handover, cut: text.length > LINE, ...why });
    return sent;
  }

  stop() {
    this.stopped = true;
    this.send({ type: "session.close" });
    // GPT-Live closes the connection after its goodbye; if it doesn't, the service does.
    setTimeout(() => this.socket.close(), 3000);
  }

  private send(event: Record<string, unknown>) {
    if (this.socket.readyState !== WebSocket.OPEN) return false;
    this.sent += 1;
    this.socket.send(JSON.stringify(event));
    return true;
  }

  /** A piece of a transcript. */
  private piece(who: "me" | "voice", event: any) {
    const fragment: Fragment = { event_id: event.event_id, delta: event.delta, start_ms: event.start_ms ?? null, end_ms: event.end_ms ?? null };
    // Every fragment is logged as it came, with its times: they show how the API cuts speech and when it delivers it.
    this.events.log(who === "me" ? "user" : "voice", "fragment", { level: "debug", text: fragment.delta, event_id: fragment.event_id, start_ms: fragment.start_ms, end_ms: fragment.end_ms, handover_pending: this.handover?.id ?? null });
    const growing = this.growing[who];
    growing.fragments.push(fragment);
    if (growing.timer) clearTimeout(growing.timer);
    // The user's words that belong to a hand-over are cut by the hand-over, not by a pause.
    if (who === "me" && this.handover) return this.settle();
    growing.timer = setTimeout(() => this.message(who, who === "me" ? this.take() : null), PAUSE[who]);
  }

  /** The wordless hand-over, if one waits for its words. */
  private take() {
    const id = this.wordless;
    this.wordless = null;
    return id;
  }

  /** Makes one message of what the user or Voice said so far. */
  private message(who: "me" | "voice", handover: string | null) {
    const growing = this.growing[who];
    if (growing.timer) clearTimeout(growing.timer);
    growing.timer = null;
    const fragments = growing.fragments.splice(0);
    const text = fragments.map((fragment) => fragment.delta).join("").trim();
    if (!text) return false;
    const starts = fragments.map((fragment) => fragment.start_ms).filter((ms): ms is number => ms !== null);
    const ends = fragments.map((fragment) => fragment.end_ms).filter((ms): ms is number => ms !== null);
    const said: Said = { who, text, start_ms: starts.length ? Math.min(...starts) : null, end_ms: ends.length ? Math.max(...ends) : null, handover };
    this.events.log(who === "me" ? "user" : "voice", "utterance", { text, start_ms: said.start_ms, end_ms: said.end_ms, handover, fragments: fragments.length });
    this.events.said(said);
    return true;
  }

  /** Voice handed the user's words over. The event has no words: they are the fragments around its time, which may still be arriving. */
  private handed(event: any) {
    const id = String(event.delegation?.id);
    this.events.log("voice", "handover", { handover: id, offset_ms: event.offset_ms ?? null, event: shape(event), words_so_far: this.growing.me.fragments.map((fragment) => fragment.delta).join(""), last_end_ms: this.growing.me.fragments.at(-1)?.end_ms ?? null });
    // An empty silent note cancels the automatic "one moment": Fast decides what the user hears.
    this.send({ type: "session.thinking.append", event_id: `quiet_${this.sent}`, delegation_id: id, content: "" });
    // A second hand-over before the first one's words were cut: the words so far go with the first.
    if (this.handover) this.cut();
    this.handover = { id, offset_ms: event.offset_ms ?? null, at: Date.now(), timer: null };
    this.settle();
  }

  /** Waits for the user's transcript to be quiet for a moment, then cuts the hand-over's words. */
  private settle() {
    const handover = this.handover!;
    if (handover.timer) clearTimeout(handover.timer);
    const left = handover.at + SETTLE_MOST - Date.now();
    handover.timer = setTimeout(() => this.cut(), Math.max(0, Math.min(SETTLE, left)));
  }

  private cut() {
    const handover = this.handover;
    if (!handover) return;
    if (handover.timer) clearTimeout(handover.timer);
    this.handover = null;
    // What Voice was still saying is closed first, so the messages stay in the order they were spoken.
    this.message("voice", null);
    const waited = Date.now() - handover.at;
    if (this.message("me", handover.id)) return this.events.log("service", "handover_cut", { level: "debug", handover: handover.id, waited_ms: waited });
    // Its words were cut into a message by a pause before it came: it is that message's hand-over.
    if (this.events.handed(handover.id)) return this.events.log("service", "handover_after_words", { handover: handover.id, waited_ms: waited });
    // No words at all: they are late, or Voice handed over a noise. The next thing the user says is this hand-over's.
    this.wordless = handover.id;
    this.events.log("service", "handover_without_words", { handover: handover.id, waited_ms: waited });
  }

  private on(event: any) {
    switch (event.type) {
      case "session.output_audio.delta":
        return this.events.audio(Buffer.from(event.delta, "base64"));
      case "session.input_transcript.delta":
        this.events.heard("me", event.delta);
        // The user speaks again: what Voice was saying is one message.
        this.message("voice", null);
        return this.piece("me", event);
      case "session.output_transcript.delta":
        this.events.heard("ai", event.delta);
        return this.piece("voice", event);
      case "session.delegation.created":
        return this.handed(event);
      case "session.commentary.appended":
      case "session.thinking.appended":
        return this.events.log("service", "append_acknowledged", { level: "debug", type: event.type, client_event_id: event.client_event_id ?? null });
      case "session.usage.updated":
      case "session.closed":
        // The seconds the session has run so far: what Voice is billed by.
        return this.events.log("voice", "voice_usage", { level: "debug", seconds: event.usage?.seconds ?? null, context_ratio: event.context_window?.usage_ratio ?? null, ...(event.type === "session.closed" ? { closed: event.reason ?? null } : {}) });
      case "error":
        console.error("GPT-Live:", event.error?.message ?? JSON.stringify(event.error));
        return this.events.log("voice", "error", { text: event.error?.message ?? JSON.stringify(event.error), event: shape(event) });
      default:
        // An event this file does not handle: logged whole, without audio, to find what the API sends.
        this.events.log("service", "unknown_event", { level: "debug", type: String(event.type), event: shape(event) });
    }
  }
}
