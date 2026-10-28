// The compositor on the CPU, for a canvas without WebGPU: a worker holds its
// own copy of each such canvas's design, fed the same stream messages, and
// paints tiles with vello_cpu off the page's main thread; the page keeps
// them — least recently used dropped first, marked stale by the damage its
// own copy of the design reports — and composites the view from them into
// a 2D canvas.

// A tile's side, in device pixels.
export const TILE = 256;
// The most tiles a canvas keeps on the CPU: more than a 4K screen at a device
// pixel ratio of 2 shows, so a view never evicts its own. The GPU keeps as
// many (`gpu::CAP`).
const CAP = 384;
const THEMES = ["light", "dark"];
// Each canvas's tiles by its id, for the worker's answers.
const byId = new Map();
let nextId = 1;
// Each tile asked of the worker, numbered.
let nextTicket = 1;
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

// One canvas's tiles on the CPU, drawn into `context`; `changed` is called
// when a tile arrives, so the canvas composites again.
export class Tiles {
  constructor(context, changed) {
    this.context = context;
    this.changed = changed;
    this.id = nextId++;
    byId.set(this.id, this);
    cpuWorker();
    // The tiles, by theme, scale, column and row, least recently used
    // first; those asked of the worker, with the damage count they were asked
    // at; how many damages there have been, and how many tiles painted.
    this.tiles = new Map();
    this.pending = new Map();
    this.damages = 0;
    this.painted = 0;
    this.closed = false;
  }

  // Another design: the worker's copy starts empty, and every tile goes.
  open() {
    worker.postMessage({ kind: "open", id: this.id });
    this.forget();
  }

  // One stream message, applied to the worker's copy too, and the damage
  // the page's copy reported.
  apply(message, damage) {
    worker.postMessage({ kind: "apply", id: this.id, message });
    this.damage(damage);
  }

  close() {
    this.closed = true;
    worker.postMessage({ kind: "close", id: this.id });
    this.forget();
    byId.delete(this.id);
  }

  // Composites the view into the 2D context: the cached tiles of this
  // theme, at any scale, moved and scaled, the view's own scale `k` on top,
  // the drawing's top-left at `(ox, oy)`; once a zoom settles, the missing or
  // stale tiles of the view are asked for, and each is swapped in when it
  // arrives. Returns how many tiles of the view are still to paint.
  draw({ size: [w, h], k, ox, oy, theme, width, height, settled }) {
    const cols = Math.ceil((w * k) / TILE);
    const rows = Math.ceil((h * k) / TILE);
    const context = this.context;
    context.clearRect(0, 0, width, height);
    const shown = [...this.tiles.values()].filter((tile) => tile.theme === theme);
    shown.sort((a, b) => (a.scale === k) - (b.scale === k) || a.scale - b.scale);
    for (const tile of shown) {
      const side = (TILE / tile.scale) * k;
      const [tx, ty] = [ox + tile.col * side, oy + tile.row * side];
      if (tx > width || ty > height || tx + side < 0 || ty + side < 0) continue;
      context.drawImage(tile.bitmap, tx, ty, side, side);
    }
    const missing = [];
    for (let row = Math.max(0, Math.floor(-oy / TILE)); row < Math.min(rows, Math.ceil((height - oy) / TILE)); row++) {
      for (let col = Math.max(0, Math.floor(-ox / TILE)); col < Math.min(cols, Math.ceil((width - ox) / TILE)); col++) {
        const key = `${theme}:${k}:${col}:${row}`;
        const tile = this.tiles.get(key);
        if (tile) {
          // In use: the most recently used goes last.
          this.tiles.delete(key);
          this.tiles.set(key, tile);
        }
        if (!tile || tile.stale) missing.push({ key, col, row });
      }
    }
    if (settled) {
      for (const { key, col, row } of missing) {
        if (this.pending.has(key)) continue;
        const ticket = nextTicket++;
        this.pending.set(key, { ticket, damages: this.damages });
        worker.postMessage({ kind: "tile", id: this.id, ticket, key, dark: theme === "dark", theme, scale: k, col, row });
      }
    }
    return missing.length;
  }

  // A tile the worker painted: kept — stale already if damage came after it
  // was asked for — and the canvas told, to composite again.
  async tilePainted({ ticket, key, theme, scale, col, row, rgba }) {
    // Only the answer to the request still pending: not one for the design
    // this canvas showed before.
    const asked = this.pending.get(key);
    if (asked?.ticket !== ticket) return;
    this.pending.delete(key);
    if (!rgba) return this.changed();
    const bitmap = await createImageBitmap(new ImageData(new Uint8ClampedArray(rgba), TILE, TILE));
    if (this.closed) return bitmap.close();
    this.tiles.get(key)?.bitmap.close();
    this.tiles.delete(key);
    this.tiles.set(key, { theme, scale, col, row, bitmap, stale: asked.damages !== this.damages });
    this.painted++;
    for (const [old, tile] of this.tiles) {
      if (this.tiles.size <= CAP) break;
      tile.bitmap.close();
      this.tiles.delete(old);
    }
    this.changed();
  }

  // Marks stale the CPU's tiles an update damaged: `flat` is the module's
  // damage — per theme, its theme, whether every tile, the drawing's
  // top-left, the number of rects and the rects.
  damage(flat) {
    if (!flat.length) return;
    this.damages++;
    for (let at = 0; at < flat.length; ) {
      const theme = THEMES[flat[at]];
      const all = flat[at + 1] === 1;
      const [x, y, n] = [flat[at + 2], flat[at + 3], flat[at + 4]];
      const rects = [];
      for (let i = 0; i < n; i++) rects.push(flat.subarray(at + 5 + 4 * i, at + 9 + 4 * i));
      at += 5 + 4 * n;
      for (const tile of this.tiles.values()) {
        if (tile.theme !== theme) continue;
        const side = TILE / tile.scale;
        const [x0, y0] = [x + tile.col * side, y + tile.row * side];
        if (all || rects.some((r) => r[0] < x0 + side && r[2] > x0 && r[1] < y0 + side && r[3] > y0)) {
          tile.stale = true;
        }
      }
    }
  }

  // Drops every tile: another design.
  forget() {
    for (const tile of this.tiles.values()) tile.bitmap.close();
    this.tiles.clear();
    this.pending.clear();
  }
}
