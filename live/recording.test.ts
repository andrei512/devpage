import { expect, test } from "bun:test";
import { mkdtempSync, readFileSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { RATE, Recording, transcriptPath } from "./recording.ts";

test("a recording is a WAV of the sound and a transcript of what was said", () => {
  const recording = new Recording(mkdtempSync(join(tmpdir(), "recording-")), "s1");
  recording.add(new Uint8Array(RATE * 2));
  recording.said({ start_ms: 100, end_ms: 600, text: "Open the roadmap" });
  recording.close();
  const wav = readFileSync(recording.path);
  expect(statSync(recording.path).size).toBe(44 + RATE * 2);
  expect(wav.toString("latin1", 0, 4)).toBe("RIFF");
  expect(wav.readUInt32LE(40)).toBe(RATE * 2);
  const transcript = JSON.parse(readFileSync(transcriptPath(recording.path), "utf8"));
  expect(transcript).toMatchObject({ session: "s1", seconds: 1, spoken_seconds: 0.5, words: 3, text: "Open the roadmap" });
});
