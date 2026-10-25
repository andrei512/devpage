# A design has one renderer

The canvas in the browser and the PNG a design's address answers come from the
same code: the design engine lays the design out once and encodes one `vello`
scene, which vello's GPU backend draws on the canvas and its CPU backend draws
as the PNG, with the same embedded typeface. Nothing else draws a design — not
the browser's own layout, SVG or HTML, and no painter of devpage's own for one
of the two.

**Why:** the PNG is what an agent checks its work against before a person sees
it. If the PNG and the canvas come from different layouts or different scenes
they drift — a line breaks differently, a shadow goes missing — and the agent
approves a drawing the person never sees. One layout and one scene leave only
the rasterizers' edge anti-aliasing to differ, which no reader can see.

**Check:** the server's PNG route and the WebAssembly module both call the
engine's one function that builds the vello scene, and neither draws anything
itself. A test paints the example designs with both backends and finds every
pixel within a small anti-aliasing tolerance of the other.
