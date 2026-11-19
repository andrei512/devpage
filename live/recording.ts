// The user's voice, saved as it is sent to the voice model: one WAV file per voice session, mono 16-bit at 24 kHz.
// Beside each WAV is its transcript, under the same name with `.json`: what the user said in that recording and when,
// so that a part can be found without listening to all of it. Nothing is recorded unless the `record` setting is on.
import { closeSync, mkdirSync, openSync, writeFileSync, writeSync } from "node:fs";
import { basename, join } from "node:path";

/** Samples per second of the sound the page sends. */
export const RATE = 24000;

/** One thing the user said. The times are where it is in the WAV, as the voice model placed it: close, not exact to the sample. */
export interface Spoken {
  start_ms: number | null;
  end_ms: number | null;
  text: string;
}

/** The transcript file of one recording. */
export interface Transcript {
  /** The WAV's file name, in the same folder. */
  recording: string;
  /** The voice session it is of: the `session` of its entries in the log. */
  session: string | null;
  /** When the recording started, ISO. */
  started: string | null;
  /** How long the WAV is. */
  seconds: number;
  /** How much of it is the user speaking: the sum of the utterances' lengths. */
  spoken_seconds: number;
  words: number;
  /** Everything the user said, one utterance per line. */
  text: string;
  utterances: Spoken[];
}

/** Where a recording's transcript is. */
export function transcriptPath(wav: string) {
  return wav.replace(/\.wav$/, ".json");
}

function writeTranscript(wav: string, session: string | null, started: string | null, seconds: number, utterances: Spoken[]) {
  const spoken = utterances.reduce((sum, said) => sum + (said.start_ms !== null && said.end_ms !== null ? Math.max(0, said.end_ms - said.start_ms) : 0), 0);
  const text = utterances.map((said) => said.text).join("\n");
  const transcript: Transcript = {
    recording: basename(wav),
    session,
    started,
    seconds: Math.round(seconds * 10) / 10,
    spoken_seconds: Math.round(spoken / 100) / 10,
    words: text.split(/\s+/).filter(Boolean).length,
    text,
    utterances,
  };
  writeFileSync(transcriptPath(wav), JSON.stringify(transcript, null, 2) + "\n");
  return transcript;
}

/** One voice session's recording, written as the sound arrives. */
export class Recording {
  private readonly file: number;
  private bytes = 0;
  readonly path: string;
  private readonly started = new Date();
  private readonly utterances: Spoken[] = [];

  /** `session` is the voice session's id, as the log has it. */
  constructor(folder: string, private readonly session: string | null = null) {
    mkdirSync(folder, { recursive: true });
    const name = this.started.toISOString().replace(/[:.]/g, "-").slice(0, 19);
    this.path = join(folder, `${name}.wav`);
    this.file = openSync(this.path, "w");
    this.header();
    this.transcript();
  }

  /** Something the user said in this recording. Its transcript is written again at once, so a session cut short keeps it. */
  said(said: Spoken) {
    this.utterances.push({ start_ms: said.start_ms, end_ms: said.end_ms, text: said.text });
    this.transcript();
  }

  /** It never throws: a transcript that cannot be written must not stop the session. */
  private transcript() {
    try {
      writeTranscript(this.path, this.session, this.started.toISOString(), this.bytes / (RATE * 2), this.utterances);
    } catch (error) {
      console.error("transcript:", error);
    }
  }

  /** More of the microphone's sound. */
  add(pcm: Uint8Array) {
    writeSync(this.file, pcm, 0, pcm.length, 44 + this.bytes);
    this.bytes += pcm.length;
    // The sizes in the header are kept right about once a second, so a file cut short still plays.
    if (this.bytes % (RATE * 2) < pcm.length) this.header();
  }

  /** The session ended: the file is whole. */
  close() {
    this.header();
    closeSync(this.file);
    this.transcript();
  }

  private header() {
    const header = Buffer.alloc(44);
    header.write("RIFF", 0);
    header.writeUInt32LE(36 + this.bytes, 4);
    header.write("WAVEfmt ", 8);
    header.writeUInt32LE(16, 16);
    header.writeUInt16LE(1, 20);
    header.writeUInt16LE(1, 22);
    header.writeUInt32LE(RATE, 24);
    header.writeUInt32LE(RATE * 2, 28);
    header.writeUInt16LE(2, 32);
    header.writeUInt16LE(16, 34);
    header.write("data", 36);
    header.writeUInt32LE(this.bytes, 40);
    writeSync(this.file, header, 0, 44, 0);
  }
}
