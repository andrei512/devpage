// Draws designs. The design engine, compiled to WebAssembly, holds each
// canvas's scene — fed by the page's one gRPC-Web watch stream, over every
// design the page shows — lays it out, hit-tests clicks and paints it: with vello on WebGPU, or with vello_cpu where the
// browser has no WebGPU (and in print mode, where a 2D canvas prints
// reliably).
//
// A canvas is `<canvas class="design" src="…">`. On the canvas page
// (`body.design-page`) it fills the window and can be zoomed and panned, and a
// navigation changes the address; embedded in a page it sits on a backdrop with a
// link to its canvas page — and, for a themed design, a theme switch of its
// own — and a navigation stays inside it. Either way a design is drawn at its
// own size, scaled down only to fit, never enlarged, in the theme its page
// shows unless the embed's own switch turned it.
// The engine, or why it couldn't load: then every canvas says so instead of
// drawing.
let Applied, Design, Gpu, failure;
try {
  // build.rs rewrites both addresses to the files' content addresses.
  const engine = await import("/__devpage/design-engine.js");
  await engine.default({ module_or_path: "/__devpage/design-engine.wasm" });
  ({ Applied, Design, Gpu } = engine);
} catch (error) {
  failure = error;
}

const print = !!document.querySelector("script[data-print]");
let gpu = null;
if (!print && !failure) {
  try {
    gpu = await Gpu.open();
  } catch (error) {
    console.info("devpage: no WebGPU, drawing designs on the CPU", error);
  }
}

// A tile's side, in device pixels.
const TILE = 256;
// How long a zoom is still before its tiles are painted at its resolution.
const SETTLE = 150;
// How many tiles the GPU paints in one frame.
const TILES_PER_FRAME = 6;
// The most tiles a canvas keeps on the CPU: more than a 4K screen at a device
// pixel ratio of 2 shows, so a view never evicts its own. The GPU keeps as
// many (`gpu::CAP`).
const CAP = 384;
const THEMES = ["light", "dark"];
const hosts = new Set();
// Each host by its id, for the worker's answers.
const byId = new Map();
let nextId = 1;
// Each tile asked of the worker, numbered.
let nextTicket = 1;
// The worker that paints tiles on the CPU, made when the first canvas that
// can't use WebGPU needs it.
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
// Every canvas a scan has started.
const started = new WeakSet();

// The prefix every address of this document is under: `/d/<id>/v/<n>` for a
// version in server mode, empty everywhere else. The watch stream is asked
// under it, and names each design by its path in the served folder, without
// it.
const mount = document.documentElement.dataset.mount ?? "";

// A design's path in the served folder, from a link relative to `base`.
function address(link, base) {
  const path = decodeURIComponent(new URL(link, base).pathname);
  return (path.startsWith(`${mount}/`) ? path.slice(mount.length) : path).replace(/^\//, "");
}

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

// The height an embed's backdrop may take: the window's — or on a slide, a
// fixed 1920 × 1080 stage scaled to the window, what the slide has left below
// the backdrop, down to its bottom padding, in the slide's own pixels.
function room(backdrop) {
  const slide = backdrop.closest(".slide");
  if (!slide) return innerHeight;
  // Where the room starts: the top of a grid or flex box the backdrop sits
  // in, which centres it there — measuring from the backdrop's own top would
  // chase its own size — else the backdrop's top.
  let from = backdrop;
  for (let at = backdrop.parentElement; at && at !== slide; at = at.parentElement) {
    if (/^(inline-)?(grid|flex)$/.test(getComputedStyle(at).display)) {
      from = at;
      break;
    }
  }
  const box = slide.getBoundingClientRect();
  const scale = box.height / slide.offsetHeight || 1;
  const top = (from.getBoundingClientRect().top - box.top) / scale;
  let padding = 0;
  for (let at = backdrop.parentElement; at && !padding; at = at === slide ? null : at.parentElement) {
    padding = parseFloat(getComputedStyle(at).paddingBottom) || 0;
  }
  return Math.max(0, slide.offsetHeight - top - padding);
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// The theme the page shows: "light" or "dark".
function pageTheme() {
  const theme = document.documentElement.dataset.theme;
  if (theme === "light" || theme === "dark") return theme;
  return matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

// The opaque color the page shows behind `element`, as 0xRRGGBB: its
// ancestors' backgrounds, translucent ones blended over those beneath.
function background(element) {
  const layers = [];
  for (let at = element; at; at = at.parentElement) {
    const [r, g, b, a = 1] = getComputedStyle(at).backgroundColor.match(/[\d.]+/g).map(Number);
    if (a > 0) layers.push([r, g, b, a]);
    if (a >= 1) break;
  }
  let [r, g, b] = [255, 255, 255];
  for (const [lr, lg, lb, la] of layers.reverse()) {
    [r, g, b] = [lr * la + r * (1 - la), lg * la + g * (1 - la), lb * la + b * (1 - la)];
  }
  return (Math.round(r) << 16) | (Math.round(g) << 8) | Math.round(b);
}

// One canvas and the design it shows.
class Host {
  constructor(canvas, path, { mode, parent = null, zoom = 1 }) {
    this.canvas = canvas;
    this.mode = mode; // "page", "embed" or "overlay"
    this.parent = parent;
    this.path = path;
    this.history = []; // an embed's own back stack
    this.view = { zoom, x: 0, y: 0, fitted: true };
    this.id = nextId++;
    byId.set(this.id, this);
    // The CPU's tiles, by theme, scale, column and row, least recently used
    // first; those asked of the worker, with the damage count they were asked
    // at; how many damages there have been, and how many tiles painted.
    this.tiles = new Map();
    this.pending = new Map();
    this.damages = 0;
    this.tilesPainted = 0;
    this.overlay = null;
    this.frame = 0;
    // The theme an embed's own switch turned it to; until then, none — it
    // follows the page. Never remembered.
    this.own = null;
    if (mode === "embed") {
      const backdrop = canvas.parentElement;
      // A canvas made again from the page's HTML comes with copies of what
      // the last one added beside it.
      for (const stale of backdrop.querySelectorAll(":scope > .design-corner, :scope > .design-problems")) {
        stale.remove();
      }
      delete backdrop.dataset.theme;
      canvas.removeAttribute("style");
      // The corner: the embed's own theme switch, which the page renders
      // hidden beside the canvas, then the link to the canvas page.
      const corner = document.createElement("div");
      corner.className = "design-corner";
      this.toggle = backdrop.querySelector(":scope > button.design-theme");
      if (this.toggle) {
        corner.append(this.toggle);
        this.toggle.addEventListener("click", () => this.switchTheme());
      }
      this.link = document.createElement("a");
      this.link.className = "design-open";
      this.link.textContent = "Open";
      this.link.title = "Open this design on its own page";
      corner.append(this.link);
      backdrop.append(corner);
      // The backdrop's room can change after the design arrives — a widget
      // mounting around it, a slide being shown: fit it again then.
      this.resized = new ResizeObserver(() => this.paint());
      this.resized.observe(canvas.parentElement);
    }
    this.attach();
    this.listen();
    hosts.add(this);
    this.open(path);
  }

  // Takes the canvas for the GPU, or for the CPU — on a fresh canvas if the
  // GPU's attempt claimed it.
  attach() {
    this.target = null;
    if (gpu) {
      try {
        this.target = gpu.target(this.canvas);
      } catch (error) {
        console.info("devpage: this canvas can't use WebGPU, drawing on the CPU", error);
        const fresh = this.canvas.cloneNode();
        this.canvas.replaceWith(fresh);
        this.canvas = fresh;
      }
    }
    this.context = this.target ? null : this.canvas.getContext("2d");
    this.worker = this.target ? null : cpuWorker();
    this.canvas.dataset.backend = this.target ? "gpu" : "cpu";
  }

  // Shows the design at `path`: a new scene, which the page's stream sends
  // once it watches this design.
  open(path) {
    this.design?.free();
    this.design = new Design();
    this.worker?.postMessage({ kind: "open", id: this.id });
    this.forget();
    this.path = path;
    this.view.fitted = true;
    this.showLink();
    // A new scene starts empty, and the stream sends a design's whole scene
    // only when it opens: open it again, even if another canvas already
    // watches this design.
    watch.key = null;
    rewatch();
  }

  // Applies one update about this host's design. A deleted design's update
  // is its `no such file` problem; it stays watched, so writing it again
  // brings it back.
  receive(message) {
    const applied = this.design.apply(message);
    if (this.worker) {
      this.worker.postMessage({ kind: "apply", id: this.id, message });
      // On the GPU the compositor takes the damage itself.
      this.damage(this.design.take_damage());
    }
    if (applied === Applied.Changed) {
      if (this.toggle) this.toggle.hidden = !this.design.themed();
      this.showProblems([]);
      this.paint();
    } else if (applied === Applied.Problems) {
      this.showProblems(this.design.problems());
      this.settle();
    }
  }

  close() {
    this.closeOverlay();
    this.resized?.disconnect();
    cancelAnimationFrame(this.frame);
    hosts.delete(this);
    // The scene lives in the module's memory, not in JavaScript's.
    this.design?.free();
    this.design = null;
    this.worker?.postMessage({ kind: "close", id: this.id });
    this.forget();
    byId.delete(this.id);
    rewatch();
  }

  // The problems bar over the last valid drawing.
  showProblems(problems) {
    const holder = this.mode === "page" ? document.body : this.canvas.parentElement;
    let bar = holder.querySelector(":scope > .design-problems");
    if (!problems.length) {
      bar?.remove();
      return;
    }
    if (!bar) {
      bar = document.createElement("ul");
      bar.className = "design-problems";
      holder.prepend(bar);
    }
    bar.replaceChildren(
      ...problems.map((problem) => {
        const item = document.createElement("li");
        item.textContent = problem;
        return item;
      }),
    );
  }

  // The theme this canvas draws its design in: an overlay's is the canvas it
  // covers; an embed's its own switch's, once turned, else the page's.
  theme() {
    if (this.mode === "overlay") return this.parent.theme();
    return this.own ?? pageTheme();
  }

  // The embed's own switch: only this embed — design, overlay and backdrop,
  // whose tokens follow its `data-theme` — turns to the other theme.
  switchTheme() {
    this.own = this.theme() === "dark" ? "light" : "dark";
    this.canvas.parentElement.dataset.theme = this.own;
    const label = this.own === "dark" ? "Switch this design to light" : "Switch this design to dark";
    this.toggle.title = label;
    this.toggle.setAttribute("aria-label", label);
    this.paint();
    this.overlay?.paint();
  }

  // The link to the canvas page, carrying the theme the embed shows.
  showLink() {
    if (this.link) this.link.href = `${mount}/${this.path}?theme=${this.theme()}`;
  }

  // The view: where the design sits and how large, in CSS pixels.
  fit() {
    const [w, h] = this.design.size();
    if (!w) return false;
    const box = this.canvas.getBoundingClientRect();
    if (this.mode === "embed") {
      // Its own size, scaled down to fit the backdrop's width and — on screen —
      // the window's height, or on a slide the room left above the slide's
      // bottom padding; the backdrop centres it.
      const backdrop = this.canvas.parentElement;
      const style = getComputedStyle(backdrop);
      const across = backdrop.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
      const down = room(backdrop) - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom);
      // Not laid out yet — hidden, or its widget still mounting.
      if (across <= 0 || down <= 0) return false;
      // Paper has no window, but a slide is the same stage in print.
      const tall = print && !backdrop.closest(".slide") ? Infinity : down / h;
      const zoom = Math.min(1, across / w, tall);
      this.canvas.style.width = `${w * zoom}px`;
      this.canvas.style.height = `${h * zoom}px`;
      Object.assign(this.view, { zoom, x: 0, y: 0 });
    } else if (this.mode === "overlay") {
      const zoom = this.parent.view.zoom;
      this.canvas.style.width = `${w * zoom}px`;
      this.canvas.style.height = `${h * zoom}px`;
      Object.assign(this.view, { zoom, x: 0, y: 0 });
    } else if (this.view.fitted) {
      const pad = 32;
      const zoom = Math.min(1, (box.width - 2 * pad) / w, (box.height - 2 * pad) / h);
      // Centered on whole pixels, so a design at 100% lands on the pixel grid.
      Object.assign(this.view, { zoom, x: Math.round((box.width - w * zoom) / 2), y: Math.round((box.height - h * zoom) / 2) });
    }
    return true;
  }

  // Paints on the next frame — once, however many changes arrive before it.
  // Until then the canvas isn't painted: what it shows is about to change.
  paint() {
    delete this.canvas.dataset.painted;
    if (this.frame) return;
    this.frame = requestAnimationFrame(() => {
      this.frame = 0;
      this.draw();
    });
  }

  // How much larger the canvas shows on the screen than its CSS size: a
  // slide's stage is a 1920 × 1080 box scaled to the window with a transform,
  // which CSS sizes don't see. 1 everywhere else.
  scaled() {
    return this.canvas.getBoundingClientRect().width / this.canvas.offsetWidth || 1;
  }

  draw() {
    if (!this.design || !this.canvas.isConnected) return;
    // The scene holds both themes: a switch is a new drawing, made here.
    this.design.set_theme(this.theme() === "dark");
    this.showLink();
    if (!this.fit()) return this.settle();
    // The bitmap matches the canvas's box on the screen, in device pixels.
    // The view is in CSS pixels, which a transform — a scaled slide — shrinks
    // on the screen: device pixels per view pixel are the screen's ratio
    // times the transform's, so the design is painted whole and crisp.
    const box = this.canvas.getBoundingClientRect();
    const width = Math.max(1, Math.round(box.width * (window.devicePixelRatio || 1)));
    const height = Math.max(1, Math.round(box.height * (window.devicePixelRatio || 1)));
    const ratio = (window.devicePixelRatio || 1) * this.scaled();
    if (this.canvas.width !== width) this.canvas.width = width;
    if (this.canvas.height !== height) this.canvas.height = height;
    // What is shown: the scale tiles are painted at — the view's in device
    // pixels, to a 1/1024th — and the drawing's top-left on a whole device
    // pixel, so a tile at the view's own scale lands pixel for pixel. Clicks
    // are taken back through the same.
    const { zoom, x, y } = this.view;
    const k = Math.max(1, Math.round(zoom * ratio * 1024)) / 1024;
    const ox = Math.round(x * ratio);
    const oy = Math.round(y * ratio);
    this.shown = { k, ox, oy };
    this.canvas.dataset.view = `${k / ratio},${ox / ratio},${oy / ratio}`;
    this.canvas.dataset.size = this.design.size().join(",");
    // The compositor places the view's cached tiles every frame; tiles still
    // to paint at this scale are painted once a zoom settles, and the canvas
    // is painted when none are left.
    const settled = performance.now() - (this.zoomed ?? 0) > SETTLE;
    const started = performance.now();
    let left;
    if (this.target) {
      const behind = background(this.canvas.parentElement);
      left = gpu.draw(this.target, this.design, width, height, k, ox, oy, behind, settled, TILES_PER_FRAME);
      this.tilesPainted = this.target.painted();
    } else {
      left = this.drawCpu(width, height, settled);
    }
    // For the tests: how long the frame took, and how many tiles the canvas
    // has painted.
    this.canvas.dataset.frame = String(performance.now() - started);
    this.canvas.dataset.tiles = String(this.tilesPainted);
    if (left === 0) this.painted();
    else if (left > 0 && !settled) {
      clearTimeout(this.settling);
      this.settling = setTimeout(() => this.paint(), SETTLE);
    } else if (left > 0 && this.target) this.paint();
  }

  // The CPU composites the view from tiles a worker paints with vello_cpu:
  // the cached tiles of this theme, at any scale, moved and scaled, the
  // view's own scale on top; once a zoom settles, the missing or stale tiles
  // of the view are asked for, and each is swapped in when it arrives.
  // Returns how many tiles of the view are still to paint.
  drawCpu(width, height, settled) {
    const [w, h] = this.design.size();
    const { k, ox, oy } = this.shown;
    const theme = this.theme();
    const cols = Math.ceil((w * k) / TILE);
    const rows = Math.ceil((h * k) / TILE);
    this.context.clearRect(0, 0, width, height);
    const shown = [...this.tiles.values()].filter((tile) => tile.theme === theme);
    shown.sort((a, b) => (a.scale === k) - (b.scale === k) || a.scale - b.scale);
    for (const tile of shown) {
      const side = (TILE / tile.scale) * k;
      const [tx, ty] = [ox + tile.col * side, oy + tile.row * side];
      if (tx > width || ty > height || tx + side < 0 || ty + side < 0) continue;
      this.context.drawImage(tile.bitmap, tx, ty, side, side);
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
        this.worker.postMessage({ kind: "tile", id: this.id, ticket, key, dark: theme === "dark", theme, scale: k, col, row });
      }
    }
    return missing.length;
  }

  // A tile the worker painted: kept — stale already if damage came after it
  // was asked for — and the view composited again.
  async tilePainted({ ticket, key, theme, scale, col, row, rgba }) {
    // Only the answer to the request still pending: not one for the design
    // this canvas showed before.
    const asked = this.pending.get(key);
    if (asked?.ticket !== ticket) return;
    this.pending.delete(key);
    if (!rgba) return this.paint();
    const bitmap = await createImageBitmap(new ImageData(new Uint8ClampedArray(rgba), TILE, TILE));
    if (!this.design) return bitmap.close();
    this.tiles.get(key)?.bitmap.close();
    this.tiles.delete(key);
    this.tiles.set(key, { theme, scale, col, row, bitmap, stale: asked.damages !== this.damages });
    this.tilesPainted++;
    for (const [old, tile] of this.tiles) {
      if (this.tiles.size <= CAP) break;
      tile.bitmap.close();
      this.tiles.delete(old);
    }
    this.paint();
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

  painted() {
    this.canvas.dataset.painted = "true";
  }

  // A design with problems and nothing valid to draw is as done as it will
  // get: its problems bar is what shows, so whoever waits for the canvas to
  // be painted — a PDF, a slide's PNG — goes on.
  settle() {
    if (this.frame || this.canvas.dataset.painted || !this.design?.problems().length) return;
    this.canvas.dataset.painted = "problems";
  }

  // From a pointer event to the design's own pixels, through any transform
  // the canvas is shown under.
  point(event) {
    const box = this.canvas.getBoundingClientRect();
    if (this.shown) {
      // In device pixels, as the compositor placed the drawing.
      const { k, ox, oy } = this.shown;
      const device = this.canvas.width / box.width || 1;
      return [((event.clientX - box.left) * device - ox) / k, ((event.clientY - box.top) * device - oy) / k];
    }
    const scale = this.scaled();
    const { zoom, x, y } = this.view;
    return [((event.clientX - box.left) / scale - x) / zoom, ((event.clientY - box.top) / scale - y) / zoom];
  }

  listen() {
    const canvas = this.canvas;
    const pointers = new Map();
    let moved = false;
    let pinch = null;
    canvas.addEventListener("pointerdown", (event) => {
      pointers.set(event.pointerId, [event.clientX, event.clientY]);
      moved = false;
      if (this.mode === "page") canvas.setPointerCapture(event.pointerId);
    });
    canvas.addEventListener("pointermove", (event) => {
      const last = pointers.get(event.pointerId);
      if (this.mode === "page" && last) {
        if (pointers.size === 2) {
          // Two fingers: zoom by how far apart they moved, about their middle.
          pointers.set(event.pointerId, [event.clientX, event.clientY]);
          const [a, b] = [...pointers.values()];
          const distance = Math.hypot(a[0] - b[0], a[1] - b[1]);
          if (pinch) this.zoomBy(distance / pinch, (a[0] + b[0]) / 2, (a[1] + b[1]) / 2);
          pinch = distance || null;
          moved = true;
          return;
        }
        const [dx, dy] = [event.clientX - last[0], event.clientY - last[1]];
        if (moved || Math.hypot(dx, dy) > 3) {
          moved = true;
          pointers.set(event.pointerId, [event.clientX, event.clientY]);
          this.panBy(dx, dy);
          canvas.style.cursor = "grabbing";
          return;
        }
      }
      canvas.style.cursor = this.design.action_at(...this.point(event)) ? "pointer" : "";
    });
    const up = (event) => {
      pointers.delete(event.pointerId);
      if (pointers.size < 2) pinch = null;
      canvas.style.cursor = "";
    };
    canvas.addEventListener("pointerup", up);
    canvas.addEventListener("pointercancel", up);
    canvas.addEventListener("click", (event) => {
      if (moved) return;
      const action = this.design.action_at(...this.point(event));
      if (action) {
        event.preventDefault();
        this.act(action);
      }
    });
    if (this.mode !== "page") return;
    // Ctrl or ⌘ and the wheel — which is also what a trackpad's pinch sends —
    // zooms about the pointer; the wheel alone pans.
    canvas.addEventListener(
      "wheel",
      (event) => {
        event.preventDefault();
        if (event.ctrlKey || event.metaKey) this.zoomBy(Math.exp(-event.deltaY * 0.01), event.clientX, event.clientY);
        else this.panBy(-event.deltaX, -event.deltaY);
      },
      { passive: false },
    );
  }

  panBy(dx, dy) {
    this.view.x += dx;
    this.view.y += dy;
    this.view.fitted = false;
    this.paint();
  }

  zoomBy(factor, clientX, clientY) {
    const box = this.canvas.getBoundingClientRect();
    const [px, py] = [clientX - box.left, clientY - box.top];
    const zoom = Math.min(64, Math.max(0.05, this.view.zoom * factor));
    const k = zoom / this.view.zoom;
    this.view.x = px - (px - this.view.x) * k;
    this.view.y = py - (py - this.view.y) * k;
    this.view.zoom = zoom;
    this.view.fitted = false;
    this.zoomed = performance.now();
    this.paint();
  }

  // Runs an interaction. The engine has already made its path an address in
  // the served folder, from the file the interaction was written in.
  act(action) {
    const [verb, ...rest] = action.trim().split(/\s+/);
    const target = rest.length ? rest.join(" ") : null;
    if (this.mode === "overlay") {
      // `close` closes this overlay; anything else closes it and runs below it.
      const parent = this.parent;
      parent.closeOverlay();
      if (verb !== "close") parent.act(target ? `${verb} ${target}` : verb);
      return;
    }
    if (verb === "navigate" && target) {
      // The canvas page keeps a theme its address named.
      if (this.mode === "page") history.pushState(null, "", `${mount}/${target}${asked ? `?theme=${asked}` : ""}`);
      else this.history.push(this.path);
      this.closeOverlay();
      this.open(target);
    } else if (verb === "back") {
      this.closeOverlay();
      if (this.mode === "page") history.back();
      else if (this.history.length) this.open(this.history.pop());
    } else if (verb === "overlay" && target) {
      this.showOverlay(target);
    }
  }

  // An overlay: the design at `path`, centered over this one, which it
  // dims, added to the page's stream.
  showOverlay(path) {
    this.closeOverlay();
    const layer = document.createElement("div");
    layer.className = "design-overlay";
    if (this.mode === "page") layer.classList.add("design-overlay-page");
    const canvas = document.createElement("canvas");
    canvas.className = "design";
    layer.append(canvas);
    (this.mode === "page" ? document.body : this.canvas.parentElement).append(layer);
    this.overlay = new Host(canvas, path, { mode: "overlay", parent: this, zoom: this.view.zoom });
    this.overlay.layer = layer;
  }

  closeOverlay() {
    if (!this.overlay) return;
    this.overlay.close();
    this.overlay.layer.remove();
    this.overlay = null;
  }
}

// Puts `message` in place of a canvas's drawing.
function fail(canvas, message) {
  canvas.dataset.failed = "true";
  const note = document.createElement("p");
  note.className = "design-failed";
  note.textContent = message;
  canvas.before(note);
}

// Starts every design canvas under `root` that isn't drawn yet, and stops
// those no longer in the page.
export function scan(root = document) {
  // An embed whose canvas left the page may be coming back made again — a
  // widget around it mounting afresh after a page refetch: what it showed,
  // and the way back, go to the next canvas of the same design.
  const carried = [];
  for (const host of [...hosts]) {
    if (host.canvas.isConnected || host.mode === "overlay") continue;
    if (host.mode === "embed") {
      carried.push({ src: host.canvas.getAttribute("src"), path: host.path, history: host.history });
    }
    host.close();
  }
  const page = document.body.classList.contains("design-page");
  for (const canvas of root.querySelectorAll("canvas.design[src]")) {
    // By element, not by its mark: a widget mounting its content copies the
    // HTML, mark and all, into a canvas no one has started.
    if (started.has(canvas)) continue;
    started.add(canvas);
    canvas.dataset.started = "true";
    if (failure) {
      console.error("devpage: the design engine didn't load", failure);
      fail(canvas, `This design can't be drawn: the design engine didn't load (${failure.message ?? failure}).`);
      continue;
    }
    // The Markdown file the embed was written in: the nearest page, chapter
    // or slide.
    const base = canvas.closest("[data-source]")?.dataset.source;
    const path = address(canvas.getAttribute("src"), new URL(base ?? location.pathname, location.origin));
    const at = page ? -1 : carried.findIndex((state) => state.src === canvas.getAttribute("src"));
    const state = at >= 0 ? carried.splice(at, 1)[0] : null;
    const host = new Host(canvas, state?.path ?? path, { mode: page ? "page" : "embed" });
    if (state) host.history = state.history;
    if (page) {
      addEventListener("popstate", () => {
        host.closeOverlay();
        host.open(address(location.pathname, location.origin));
      });
    }
  }
}

addEventListener("resize", () => {
  for (const host of hosts) host.paint();
});

// A theme switch changes the designs' colors and the page's background,
// which the GPU paints behind a design: every canvas paints again — but an
// embed its own switch turned keeps its theme. On a page the page script
// switches the theme; the canvas page has no page script, so this one runs
// its switch — the same choice, kept under the same name.
new MutationObserver(() => {
  for (const host of hosts) host.paint();
}).observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });

// The theme the canvas page's address names, `?theme=light` or `dark`: it
// shows for this view, over the remembered choice, which it leaves alone.
const asked = (() => {
  const theme = new URLSearchParams(location.search).get("theme");
  return theme === "light" || theme === "dark" ? theme : null;
})();

if (document.body.classList.contains("design-page")) {
  const system = matchMedia("(prefers-color-scheme: dark)");
  const saved = () => {
    try {
      const theme = localStorage.getItem("devpage-theme");
      return theme === "light" || theme === "dark" ? theme : null;
    } catch {
      return null;
    }
  };
  let chosen = asked;
  const apply = (theme = chosen ?? saved()) => {
    document.documentElement.dataset.theme = theme || (system.matches ? "dark" : "light");
    const label = document.documentElement.dataset.theme === "dark" ? "Switch to light" : "Switch to dark";
    for (const button of document.querySelectorAll(".theme-toggle")) {
      button.title = label;
      button.setAttribute("aria-label", label);
    }
  };
  apply();
  system.addEventListener("change", () => apply());
  document.addEventListener("click", (event) => {
    if (!event.target.closest?.(".theme-toggle")) return;
    const theme = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
    try {
      localStorage.setItem("devpage-theme", theme);
    } catch {
      // Storage may be blocked; the switch still holds for this page.
    }
    chosen = theme;
    apply(theme);
  });
}

// The page's one watch stream, over every design its canvases show. A
// browser keeps at most six connections to devpage open, so the page never
// holds more than this and its event stream.
const watch = { key: null, controller: null, pending: false };

// The designs to watch: each shown design once.
function watched() {
  const paths = new Set();
  for (const host of hosts) if (host.canvas.isConnected) paths.add(host.path);
  return [...paths].sort();
}

// Opens the stream again once the set of designs has changed — after
// whatever else this turn changes, so a page swap reopens it once.
function rewatch() {
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
      const path = addressed(message);
      for (const host of hosts) {
        if (host.path === path && host.canvas.isConnected) host.receive(message);
      }
    }
  }
}

// A hidden tab keeps no stream open; shown again, the page reopens it and
// gets each whole scene.
document.addEventListener("visibilitychange", () => {
  if (document.hidden) {
    watch.controller?.abort();
    watch.controller = null;
  } else {
    connect();
  }
});

scan();
