# A design has one renderer

The canvas in the browser and the PNG a design's address answers are drawn by
the same code: the design engine's layout and painter, compiled once natively
and once to WebAssembly, painting on the CPU with the same embedded typeface.
No design is ever drawn by the browser's own layout, by SVG or HTML, or by a
second painter for one of the two.

**Why:** the PNG is what an agent checks its work against before a person sees
it. If the PNG and the canvas come from different renderers they drift — a line
breaks differently, a shadow is lighter — and the agent approves a drawing the
person never sees. One renderer makes "the agent saw it" and "the person saw
it" the same claim.

**Check:** both the server's PNG route and the WebAssembly module call the
engine's one paint function, and neither has drawing code of its own. A test
paints the example designs natively and through the WebAssembly module and
finds the buffers identical.
