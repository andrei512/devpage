// The compositor on the CPU, for a canvas without WebGPU: it carries out the
// frame plan the canvas's design holds in the module. A worker holds its own
// copy of each such canvas's design, fed the same stream messages, and paints
// the tiles the plan asks for with vello_cpu off the page's main thread; the
// page keeps each painting by the ticket it was asked under, frees the ones
// the plan lets go, and draws the ones it shows into a 2D canvas.

// A tile's side, in device pixels.
export const TILE = 256;
// Each canvas's tiles by its id, for the worker's answers.
const byId = new Map();
let nextId = 1;
// The worker, made when the first canvas that can't use WebGPU needs it.
let worker = null;
function cpuWorker() {
  if (!worker) {
    worker = new Worker("/__devpage/design-worker.js", { type: "module" });
    worker.onmessage = ({ data }) => {
      if (data.kind === "tile") byId.get(data.id)?.tilePainted(data);
    };
  }
  return worker;
}

// One canvas's paintings on the CPU, drawn into `context`; `changed` is
// called when one arrives, so the canvas composites again.
export class Tiles {
  constructor(context, changed) {
    this.context = context;
    this.changed = changed;
    this.id = nextId++;
    byId.set(this.id, this);
    cpuWorker();
    // The paintings by ticket, and the design whose plan asked for them.
    this.bitmaps = new Map();
    this.design = null;
    // For the tests: how many tiles were painted; the latest, in the order
    // they arrived, as `scale:col,row`; how many are asked of the worker
    // and not yet answered, and the most ever at once.
    this.painted = 0;
    this.order = [];
    this.asked = 0;
    this.inflight = 0;
    this.closed = false;
  }

  // Another design: the worker's copy starts empty, and every painting goes.
  open(design) {
    this.design = design;
    worker.postMessage({ kind: "open", id: this.id });
    this.forget();
  }

  // One stream message, applied to the worker's copy too.
  apply(message) {
    worker.postMessage({ kind: "apply", id: this.id, message });
  }

  close() {
    this.closed = true;
    worker.postMessage({ kind: "close", id: this.id });
    this.forget();
    byId.delete(this.id);
  }

  // Carries out a frame's `work` from the plan in a canvas `width` ×
  // `height`: frees what it lets go, draws what it shows where it lands, and
  // asks the worker for what it requests. Returns how many tiles of the view
  // are still to paint.
  draw(work, width, height) {
    for (const ticket of work.evict) {
      this.bitmaps.get(ticket)?.close();
      this.bitmaps.delete(ticket);
    }
    this.context.clearRect(0, 0, width, height);
    const draw = work.draw;
    for (let at = 0; at < draw.length; at += 5) {
      const bitmap = this.bitmaps.get(draw[at]);
      const [x0, y0, x1, y1] = draw.subarray(at + 1, at + 5);
      if (bitmap) this.context.drawImage(bitmap, x0, y0, x1 - x0, y1 - y0);
    }
    const request = work.request;
    for (let at = 0; at < request.length; at += 5) {
      const [ticket, theme, scale, col, row] = request.subarray(at, at + 5);
      this.inflight = Math.max(this.inflight, ++this.asked);
      worker.postMessage({ kind: "tile", id: this.id, ticket, dark: theme === 1, scale, col, row });
    }
    return work.missing;
  }

  // A tile the worker painted, or couldn't: kept if the plan still asks for
  // it, and the canvas told, to composite again.
  async tilePainted({ ticket, scale, col, row, rgba }) {
    this.asked--;
    if (this.closed) return;
    if (!rgba) {
      this.design.missed(ticket);
      return this.changed();
    }
    const bitmap = await createImageBitmap(new ImageData(new Uint8ClampedArray(rgba), TILE, TILE));
    if (this.closed || !this.design.arrived(ticket)) return bitmap.close();
    this.bitmaps.set(ticket, bitmap);
    this.painted++;
    this.order.push(`${scale}:${col},${row}`);
    if (this.order.length > 256) this.order.shift();
    this.changed();
  }

  // Frees every painting: another design, or none.
  forget() {
    for (const bitmap of this.bitmaps.values()) bitmap.close();
    this.bitmaps.clear();
  }
}
