// The page's one watch stream: a gRPC-Web call over every design the page
// shows, read frame by frame, each update handed to the canvases showing its
// design. A browser keeps at most six connections to devpage open, so the
// page never holds more than this and its event stream. A hidden tab keeps
// no stream open; shown again, the page reopens it and gets each whole scene.

// A varint, as protobuf writes one.
function varint(n) {
  const bytes = [];
  for (; ; n >>>= 7) {
    if (n < 0x80) { bytes.push(n); return bytes; }
    bytes.push((n & 0x7f) | 0x80);
  }
}

// The watch request, framed for gRPC-Web: field 1, repeated, each design's
// address.
function watchRequest(paths) {
  const fields = [];
  for (const path of paths) {
    const name = new TextEncoder().encode(path);
    fields.push(0x0a, ...varint(name.length), ...name);
  }
  const body = new Uint8Array(5 + fields.length);
  new DataView(body.buffer).setUint32(1, fields.length);
  body.set(fields, 5);
  return body;
}

// The design an update is about (field 4), read from the update's top
// level; the engine decodes the rest.
function addressed(message) {
  let at = 0;
  const next = () => {
    let n = 0;
    for (let shift = 0; ; shift += 7) {
      const byte = message[at++];
      n += (byte & 0x7f) * 2 ** shift;
      if (byte < 0x80) return n;
    }
  };
  let path = "";
  while (at < message.length) {
    const key = next();
    const field = Math.floor(key / 8);
    const type = key & 7;
    if (type === 0) next();
    else if (type === 2) {
      const length = next();
      if (field === 4) path = new TextDecoder().decode(message.subarray(at, at + length));
      at += length;
    } else if (type === 1) at += 8;
    else if (type === 5) at += 4;
    else break; // not an update this script knows
  }
  return path;
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// The page's one watch stream, over every design its canvases show. A
// browser keeps at most six connections to devpage open, so the page never
// holds more than this and its event stream.
const watch = { key: null, controller: null, pending: false };
// What the stream is told by the page: the designs it shows, and where each
// update goes.
let shown = () => [];
let deliver = () => {};
let mount = "";
let print = false;

// Starts the page's stream: `options.shown()` names the designs the page
// shows, `options.deliver(path, message)` hands an update to whatever shows
// its design; every address is under `options.mount`, and in print mode a
// dropped stream isn't reopened.
export function start(options) {
  ({ shown, deliver, mount, print } = options);
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) {
      watch.controller?.abort();
      watch.controller = null;
    } else {
      connect();
    }
  });
}

// The designs to watch: each shown design once.
function watched() {
  return [...new Set(shown())].sort();
}

// Opens the stream again even if the set of designs is the same: a canvas
// starting on a design needs its whole scene, which the stream sends only
// when it opens.
export function reopen() {
  watch.key = null;
  rewatch();
}

// Opens the stream again once the set of designs has changed — after
// whatever else this turn changes, so a page swap reopens it once.
export function rewatch() {
  if (watch.pending) return;
  watch.pending = true;
  queueMicrotask(() => {
    watch.pending = false;
    connect();
  });
}

function connect() {
  const paths = watched();
  const key = paths.join("\n");
  if (key === watch.key && watch.controller) return;
  watch.controller?.abort();
  watch.controller = null;
  watch.key = key;
  if (!paths.length || document.hidden) return;
  const controller = new AbortController();
  watch.controller = controller;
  stream(paths, controller);
}

// Reads the stream until the page closes it, reopening it — and getting each
// whole scene again — whenever devpage drops it. A stream devpage ended
// itself, with its trailers, is over: devpage refused it.
async function stream(paths, controller) {
  while (!controller.signal.aborted) {
    try {
      if (await read(paths, controller.signal)) break;
    } catch {
      // devpage stopped, or the page closed the stream: try again below.
    }
    if (controller.signal.aborted || print) return;
    await sleep(1000);
  }
  if (watch.controller === controller) watch.controller = null;
}

// Hands each update to the canvases showing its design; true when the stream
// ended with devpage's trailers.
async function read(paths, signal) {
  const response = await fetch(`${mount}/devpage.design.v1.Designs/Watch`, {
    method: "POST",
    headers: { "content-type": "application/grpc-web+proto", "x-grpc-web": "1" },
    body: watchRequest(paths),
    signal,
  });
  const reader = response.body.getReader();
  let buffer = new Uint8Array(0);
  let ended = false;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) return ended;
    const joined = new Uint8Array(buffer.length + value.length);
    joined.set(buffer);
    joined.set(value, buffer.length);
    buffer = joined;
    // gRPC-Web frames: a flag byte, a 4-byte length, the message.
    while (buffer.length >= 5) {
      const n = new DataView(buffer.buffer, buffer.byteOffset).getUint32(1);
      if (buffer.length < 5 + n) break;
      const flag = buffer[0];
      const message = buffer.slice(5, 5 + n);
      buffer = buffer.slice(5 + n);
      if (flag & 0x80) {
        ended = true;
        continue;
      }
      if (signal.aborted) return false;
      deliver(addressed(message), message);
    }
  }
}
