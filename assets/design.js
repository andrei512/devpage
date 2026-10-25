// Draws designs: the design engine, compiled to WebAssembly, holds the scene
// the gRPC-Web watch stream sends and paints it onto a canvas.
(() => {
  const engine = WebAssembly.instantiateStreaming(fetch('/__devpage/design.wasm'), {})
    .then(({ instance }) => instance.exports);

  // One canvas: its own module instance, so each holds its own scene.
  async function draw(canvas, path) {
    const { instance } = await WebAssembly.instantiateStreaming(fetch('/__devpage/design.wasm'), {});
    const e = instance.exports;
    const ctx = canvas.getContext('2d');
    let painted = false;
    const paint = () => {
      const scale = window.devicePixelRatio || 1;
      const out = e.paint_scene(scale);
      if (!out) return;
      const view = new DataView(e.memory.buffer);
      const w = view.getUint32(out, true), h = view.getUint32(out + 4, true);
      const rgba = new Uint8ClampedArray(e.memory.buffer, out + 8, w * h * 4);
      // tiny-skia is premultiplied, ImageData is not: unpremultiply a copy.
      const data = new Uint8ClampedArray(rgba);
      for (let i = 0; i < data.length; i += 4) {
        const a = data[i + 3];
        if (a && a < 255) {
          data[i] = data[i] * 255 / a; data[i + 1] = data[i + 1] * 255 / a; data[i + 2] = data[i + 2] * 255 / a;
        }
      }
      e.dealloc(out, 8 + w * h * 4);
      canvas.width = w; canvas.height = h;
      canvas.style.width = `${w / scale}px`;
      ctx.putImageData(new ImageData(data, w, h), 0, 0);
      painted = true;
    };
    canvas.onclick = (event) => {
      const r = canvas.getBoundingClientRect();
      const factor = canvas.width / (window.devicePixelRatio || 1) / r.width;
      const out = e.click((event.clientX - r.left) * factor, (event.clientY - r.top) * factor);
      if (!out) return;
      const n = new DataView(e.memory.buffer).getUint32(out, true);
      const action = new TextDecoder().decode(new Uint8Array(e.memory.buffer, out + 4, n));
      if (action === 'back') history.back();
      else if (action.startsWith('navigate ')) {
        const target = new URL(action.slice(9).trim(), new URL(path, location.origin));
        location.href = target.pathname;
      }
    };
    // The watch stream: gRPC-Web frames, 5-byte headers, off the response body.
    const name = new TextEncoder().encode(path.replace(/^\//, ''));
    const message = new Uint8Array([0x0a, name.length, ...name]);
    const body = new Uint8Array(5 + message.length);
    new DataView(body.buffer).setUint32(1, message.length);
    body.set(message, 5);
    const response = await fetch('/devpage.design.v1.Designs/Watch', {
      method: 'POST',
      headers: { 'content-type': 'application/grpc-web+proto', 'x-grpc-web': '1' },
      body,
    });
    const reader = response.body.getReader();
    let buffer = new Uint8Array(0);
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      const joined = new Uint8Array(buffer.length + value.length);
      joined.set(buffer); joined.set(value, buffer.length);
      buffer = joined;
      while (buffer.length >= 5) {
        const n = new DataView(buffer.buffer, buffer.byteOffset).getUint32(1);
        if (buffer.length < 5 + n) break;
        const flag = buffer[0], frame = buffer.slice(5, 5 + n);
        buffer = buffer.slice(5 + n);
        if (flag & 0x80) continue; // trailers
        const ptr = e.alloc(frame.length);
        new Uint8Array(e.memory.buffer, ptr, frame.length).set(frame);
        const changed = e.apply(ptr, frame.length);
        e.dealloc(ptr, frame.length);
        if (changed === 1) paint();
      }
    }
    void painted;
  }

  void engine;
  for (const canvas of document.querySelectorAll('canvas[data-design]')) {
    draw(canvas, canvas.dataset.design);
  }
})();
