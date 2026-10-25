// Draws designs. The design engine, compiled to WebAssembly, holds each
// canvas's scene — fed by a gRPC-Web watch stream — lays it out, hit-tests
// clicks and paints it: with vello on WebGPU, or with vello_cpu where the
// browser has no WebGPU (and in print mode, where a 2D canvas prints
// reliably).
//
// A canvas is `<canvas class="design" src="…">`. On the canvas page
// (`body.design-page`) it fills the window and can be zoomed and panned, and a
// navigation changes the address; embedded in a page it is drawn at the page's
// width and a navigation stays inside it.
// The engine, or why it couldn't load: then every canvas says so instead of
// drawing.
let Applied, Design, Gpu, failure;
try {
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

// The largest bitmap the CPU paints, per side, however far a reader zooms in.
const MAX_BITMAP = 8192;
const hosts = new Set();

// A design's address in the served folder, from a link relative to `base`.
function address(link, base) {
  return decodeURIComponent(new URL(link, base).pathname).replace(/^\//, "");
}

// The watch request: one string field, the design's address.
function watchRequest(path) {
  const name = new TextEncoder().encode(path);
  const length = [];
  for (let n = name.length; ; n >>>= 7) {
    if (n < 0x80) { length.push(n); break; }
    length.push((n & 0x7f) | 0x80);
  }
  const message = new Uint8Array([0x0a, ...length, ...name]);
  const body = new Uint8Array(5 + message.length);
  new DataView(body.buffer).setUint32(1, message.length);
  body.set(message, 5);
  return body;
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

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
    this.cpu = null; // the CPU's last painting: { scale, bitmap }
    this.overlay = null;
    this.frame = 0;
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
    this.canvas.dataset.backend = this.target ? "gpu" : "cpu";
  }

  // Shows the design at `path`: a new scene from a new stream.
  open(path) {
    this.controller?.abort();
    this.design?.free();
    this.design = new Design();
    this.path = path;
    this.cpu = null;
    this.view.fitted = true;
    this.stream();
  }

  // Reads the watch stream until the host closes it, reopening it — and
  // getting the whole scene again — whenever devpage drops it. A stream
  // devpage ended itself, with its trailers, is over: the design was deleted.
  async stream() {
    const controller = new AbortController();
    this.controller = controller;
    const design = this.design;
    while (!controller.signal.aborted && this.canvas.isConnected) {
      try {
        if (await this.read(design, controller.signal)) return;
      } catch {
        // devpage stopped, or the stream was closed: try again below.
      }
      if (controller.signal.aborted || print) return;
      await sleep(1000);
    }
  }

  // Applies the stream's messages; true when it ended with devpage's trailers.
  async read(design, signal) {
    const response = await fetch("/devpage.design.v1.Designs/Watch", {
      method: "POST",
      headers: { "content-type": "application/grpc-web+proto", "x-grpc-web": "1" },
      body: watchRequest(this.path),
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
        if (design !== this.design) continue; // a design no longer shown
        const applied = design.apply(message);
        if (applied === Applied.Changed) {
          this.showProblems([]);
          this.cpu = null;
          this.paint();
        } else if (applied === Applied.Problems) {
          this.showProblems(design.problems());
        }
      }
    }
  }

  close() {
    this.controller?.abort();
    this.overlay?.close();
    hosts.delete(this);
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

  // The view: where the design sits and how large, in CSS pixels.
  fit() {
    const [w, h] = this.design.size();
    if (!w) return false;
    const box = this.canvas.getBoundingClientRect();
    if (this.mode === "embed") {
      // At the page's width, as tall as the design is at that width.
      const zoom = box.width / w;
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
  paint() {
    if (this.frame) return;
    this.frame = requestAnimationFrame(() => {
      this.frame = 0;
      this.draw();
    });
  }

  draw() {
    if (!this.canvas.isConnected || !this.fit()) return;
    const ratio = window.devicePixelRatio || 1;
    const box = this.canvas.getBoundingClientRect();
    const width = Math.max(1, Math.round(box.width * ratio));
    const height = Math.max(1, Math.round(box.height * ratio));
    if (this.canvas.width !== width) this.canvas.width = width;
    if (this.canvas.height !== height) this.canvas.height = height;
    const { zoom, x, y } = this.view;
    this.canvas.dataset.view = `${zoom},${x},${y}`;
    this.canvas.dataset.size = this.design.size().join(",");
    if (this.target) {
      const behind = background(this.canvas.parentElement);
      if (gpu.draw(this.target, this.design, width, height, zoom * ratio, x * ratio, y * ratio, behind)) this.painted();
      return;
    }
    this.drawCpu(ratio, width, height);
  }

  // The CPU paints the whole design once per zoom level; a pan only moves the
  // painting.
  drawCpu(ratio, width, height) {
    const [w, h] = this.design.size();
    const { zoom, x, y } = this.view;
    const scale = Math.min(zoom * ratio, MAX_BITMAP / w, MAX_BITMAP / h);
    const show = (bitmap) => {
      this.context.clearRect(0, 0, width, height);
      this.context.drawImage(bitmap, x * ratio, y * ratio, w * zoom * ratio, h * zoom * ratio);
    };
    if (this.cpu) show(this.cpu.bitmap);
    if (this.cpu?.scale === scale) return this.painted();
    clearTimeout(this.repaint);
    // While zooming, the last painting is stretched until the zoom settles.
    this.repaint = setTimeout(
      async () => {
        const pixels = this.design.paint(scale);
        if (!pixels) return;
        const image = new ImageData(new Uint8ClampedArray(pixels.take()), pixels.width, pixels.height);
        pixels.free();
        this.cpu = { scale, bitmap: await createImageBitmap(image) };
        show(this.cpu.bitmap);
        this.painted();
      },
      this.cpu ? 120 : 0,
    );
  }

  painted() {
    this.canvas.dataset.painted = "true";
  }

  // From a pointer event to the design's own pixels.
  point(event) {
    const box = this.canvas.getBoundingClientRect();
    const { zoom, x, y } = this.view;
    return [(event.clientX - box.left - x) / zoom, (event.clientY - box.top - y) / zoom];
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
      if (this.mode === "page") history.pushState(null, "", `/${target}`);
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

  // An overlay: the design at `path`, centered over this one on a dimmed
  // backdrop, with its own stream.
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
    this.overlay.design.free();
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
  for (const host of [...hosts]) if (!host.canvas.isConnected && host.mode !== "overlay") host.close();
  const page = document.body.classList.contains("design-page");
  for (const canvas of root.querySelectorAll("canvas.design[src]")) {
    if (canvas.dataset.started) continue;
    canvas.dataset.started = "true";
    if (failure) {
      console.error("devpage: the design engine didn't load", failure);
      fail(canvas, `This design can't be drawn: the design engine didn't load (${failure.message ?? failure}).`);
      continue;
    }
    const base = canvas.closest("[data-source]")?.dataset.source;
    const path = address(canvas.getAttribute("src"), new URL(base ?? location.pathname, location.origin));
    const host = new Host(canvas, path, { mode: page ? "page" : "embed" });
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

// A hidden tab keeps no streams open; shown again, each design reopens its
// stream and gets its whole scene.
document.addEventListener("visibilitychange", () => {
  for (const host of hosts) {
    if (document.hidden) host.controller?.abort();
    else host.stream();
  }
});

scan();
