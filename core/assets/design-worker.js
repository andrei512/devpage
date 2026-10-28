// Paints designs' tiles on the CPU, off the page's main thread, for a page
// without WebGPU. It holds its own copy of each canvas's design, fed the same
// watch-stream messages as the page's, and answers each tile asked for with
// its pixels.
// Messages that arrive while the engine loads wait for it, in order.
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
    designs.get(data.id)?.apply(data.message);
  } else if (data.kind === "theme") {
    designs.get(data.id)?.set_theme(data.dark);
  } else if (data.kind === "tile") {
    const design = designs.get(data.id);
    const pixels = design?.paint_tile(data.scale, data.col, data.row);
    if (!pixels) return postMessage({ ...data, rgba: null });
    const rgba = pixels.take();
    pixels.free();
    postMessage({ ...data, rgba }, [rgba.buffer]);
  }
}
for (const data of early.splice(0)) handle(data);
onmessage = ({ data }) => handle(data);
