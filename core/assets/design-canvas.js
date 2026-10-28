// A design's canvas: `<canvas class="design" src="…">`, the view it shows
// — fitted, zoomed and panned — the pointer and the interactions it runs,
// its overlays, and on an embed the backdrop's corner and theme switch. It
// draws through the compositor: the module's on the GPU, `Tiles` on the CPU.

import { reopen, rewatch } from "/__devpage/design-stream.js";
import { Tiles } from "/__devpage/design-tiles.js";

// How long a zoom is still before its tiles are painted at its resolution.
const SETTLE = 150;
// How many tiles the GPU paints in one frame.
const TILES_PER_FRAME = 6;

// What the page gives every canvas: the engine's `Design` and `Applied`, the
// GPU or `null`, print mode, and the document's mount.
let Design, Applied, gpu, print, mount;
export function configure(options) {
  ({ Design, Applied, gpu, print, mount } = options);
}

// Every canvas started and not yet closed.
export const hosts = new Set();

// The theme the canvas page's address names, `?theme=light` or `dark`: it
// shows for this view, over the remembered choice, which it leaves alone.
export const asked = (() => {
  const theme = new URLSearchParams(location.search).get("theme");
  return theme === "light" || theme === "dark" ? theme : null;
})();

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
export class Host {
  constructor(canvas, path, { mode, parent = null, zoom = 1 }) {
    this.canvas = canvas;
    this.mode = mode; // "page", "embed" or "overlay"
    this.parent = parent;
    this.path = path;
    this.history = []; // an embed's own back stack
    this.view = { zoom, x: 0, y: 0, fitted: true };
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
    // Without the GPU, the CPU's tiles, drawn into a 2D context.
    this.tiles = this.target ? null : new Tiles(this.canvas.getContext("2d"), () => this.paint());
    this.canvas.dataset.backend = this.target ? "gpu" : "cpu";
  }

  // Shows the design at `path`: a new scene, which the page's stream sends
  // once it watches this design.
  open(path) {
    this.design?.free();
    this.design = new Design();
    this.tiles?.open();
    this.path = path;
    this.view.fitted = true;
    this.showLink();
    // A new scene starts empty, and the stream sends a design's whole scene
    // only when it opens: open it again, even if another canvas already
    // watches this design.
    reopen();
  }

  // Applies one update about this host's design. A deleted design's update
  // is its `no such file` problem; it stays watched, so writing it again
  // brings it back.
  receive(message) {
    const applied = this.design.apply(message);
    // On the GPU the compositor takes the damage itself.
    this.tiles?.apply(message, this.design.take_damage());
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
    this.tiles?.close();
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
    } else {
      const needed = this.design.view_tiles(width, height, k, ox, oy);
      left = this.tiles.draw({ needed, k, ox, oy, theme: this.theme(), width, height, settled, budget: TILES_PER_FRAME });
    }
    // For the tests: how long the frame took, and how many tiles the canvas
    // has painted.
    this.canvas.dataset.frame = String(performance.now() - started);
    this.canvas.dataset.tiles = String(this.target ? this.target.painted() : this.tiles.painted);
    if (this.tiles) {
      this.canvas.dataset.order = this.tiles.order.join(" ");
      this.canvas.dataset.inflight = String(this.tiles.inflight);
    }
    if (left === 0) this.painted();
    else if (left > 0 && !settled) {
      clearTimeout(this.settling);
      this.settling = setTimeout(() => this.paint(), SETTLE);
    } else if (left > 0 && this.target) this.paint();
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
