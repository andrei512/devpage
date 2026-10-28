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
// The modules it is made of: the watch stream, the canvas, and on the CPU the
// compositor's tiles.
import { start } from "/__devpage/design-stream.js";
import { Host, asked, configure, hosts } from "/__devpage/design-canvas.js";

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

configure({ Design, Applied, gpu, print, mount });
start({
  mount,
  print,
  shown: () => [...hosts].filter((host) => host.canvas.isConnected).map((host) => host.path),
  deliver: (path, message) => {
    for (const host of hosts) {
      if (host.path === path && host.canvas.isConnected) host.receive(message);
    }
  },
});

scan();
