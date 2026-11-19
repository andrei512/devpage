import { SessionStore } from "./store.ts";
// What the live session costs. The prices are here, the numbers are in the log: every model call of Fast with its
// tokens, the seconds each voice session ran, and what each turn of a slow agent would cost at API prices.
// `bun cost.ts`, run in the project's folder, prints the sum per day.
import { stateOf } from "./home.ts";

/** Dollars per million tokens on the standard tier (developers.openai.com/api/docs/pricing). */
const PRICES: Record<string, { input: number; cached: number; output: number }> = {
  "gpt-6.1-sol": { input: 2, cached: 0.1, output: 10 },
  "gpt-6.2-sol": { input: 2, cached: 0.1, output: 10 },
  "gpt-6-luna": { input: 0.1, cached: 0.01, output: 0.5 },
};

/** The priority tier is billed at twice the standard one. The API names it `priority` or `fast`. */
const PRIORITY = 2;

/** Voice is billed by the time the session is on. */
const VOICE_PER_MINUTE = 0.05;

export interface Tokens {
  input_tokens: number | null;
  cached_tokens: number | null;
  output_tokens: number | null;
}

/** What one model call cost, in dollars. Null for a model with no price here. */
export function price(model: string, tier: string | null, tokens: Tokens) {
  const name = Object.keys(PRICES).find((known) => model.startsWith(known));
  if (!name) return null;
  const per = PRICES[name];
  const cached = tokens.cached_tokens ?? 0;
  const fresh = (tokens.input_tokens ?? 0) - cached;
  const standard = (fresh * per.input + cached * per.cached + (tokens.output_tokens ?? 0) * per.output) / 1_000_000;
  return standard * (tier === "priority" || tier === "fast" ? PRIORITY : 1);
}

if (import.meta.main) {
  const days = Number(process.argv[2] ?? 7);
  const entries = new SessionStore(stateOf(process.cwd()), "read").logs(days);
  interface Day {
    voice: Map<string, number>;
    calls: number;
    fast: number;
    unpriced: number;
    turns: number;
    agents: number;
  }
  const byDay = new Map<string, Day>();
  for (const entry of entries) {
    const date = new Date(entry.ts).toLocaleDateString("en-CA");
    let day = byDay.get(date);
    if (!day) byDay.set(date, (day = { voice: new Map(), calls: 0, fast: 0, unpriced: 0, turns: 0, agents: 0 }));
    const more = entry as any;
    // The API reports a voice session's seconds as a running total: the last one counts.
    const seconds = entry.kind === "voice_usage" ? more.seconds : more.event?.usage?.seconds;
    if (typeof seconds === "number" && entry.session) day.voice.set(entry.session, Math.max(seconds, day.voice.get(entry.session) ?? 0));
    if (entry.who === "fast" && entry.kind === "model") {
      const cost = price(String(more.model), more.tier ?? null, more);
      day.calls++;
      if (cost === null) day.unpriced++;
      else day.fast += cost;
    }
    if (entry.who === "agent" && typeof more.cost_usd === "number") {
      day.turns++;
      day.agents += more.cost_usd;
    }
  }
  const dollars = (value: number) => `$${value.toFixed(value < 1 ? 3 : 2)}`;
  let paid = 0;
  for (const [date, day] of byDay) {
    const seconds = [...day.voice.values()].reduce((sum, value) => sum + value, 0);
    const voice = (seconds / 60) * VOICE_PER_MINUTE;
    paid += voice + day.fast;
    console.log(date);
    console.log(`  Voice   ${dollars(voice)}  ${(seconds / 60).toFixed(1)} minutes in ${day.voice.size} sessions`);
    console.log(`  Fast    ${dollars(day.fast)}  ${day.calls} model calls${day.unpriced ? `, ${day.unpriced} of them on a model with no price in cost.ts` : ""}`);
    console.log(`  Agents  on the subscription: ${day.turns} turns, ${dollars(day.agents)} at API prices`);
    console.log(`  Billed  ${dollars(voice + day.fast)}`);
  }
  console.log(`Billed in ${byDay.size} days: ${dollars(paid)}`);
}
