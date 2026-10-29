// Paints designs' tiles on the CPU, off the page's main thread, for a page
// without WebGPU. It holds its own copy of each canvas's design, fed the same
// watch-stream messages as the page's, and answers each tile asked for with
// its pixels.
// Messages that arrive while the engine loads wait for it, in order: a
// module worker drops a message that arrives during its top-level await if no
// handler is set yet.
const early = [];
onmessage = ({ data }) => early.push(data);
const engine = await import("/__devpage/design-engine.js");
await engine.default({ module_or_path: "/__devpage/design-engine.wasm" });
const designs = new Map();

function handle(data) {
  if (data.kind === "open") {
    designs.get(data.id)?.free();
    designs.set(data.id, new engine.Design());
  } else if (data.kind === "close") {
    designs.get(data.id)?.free();
    designs.delete(data.id);
  } else if (data.kind === "apply") {
    // The page's own copy of the design says which tiles it made stale.
    designs.get(data.id)?.apply(data.message);
  } else if (data.kind === "tile") {
    const design = designs.get(data.id);
    // Each tile in the theme it was asked for: the drawing in the other
    // theme is kept, so a switch back makes nothing.
    design?.set_theme(data.dark);
    const pixels = design?.paint_tile(data.scale, data.col, data.row);
    if (!pixels) return postMessage({ ...data, rgba: null });
    const rgba = pixels.take();
    pixels.free();
    postMessage({ ...data, rgba }, [rgba.buffer]);
  }
}
for (const data of early.splice(0)) handle(data);
onmessage = ({ data }) => handle(data);
