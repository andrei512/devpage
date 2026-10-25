# A design has one renderer

The canvas in the browser and the PNG a design's address answers come from the
same code: the design engine lays the design out once and paints it into one
display list, which a GPU backend (`vello`) replays on the canvas and a CPU
backend (`vello_cpu`) replays as the PNG, with the same embedded typeface. A
backend only translates commands; it decides nothing about the drawing. Nothing else draws a design — not
the browser's own layout, SVG or HTML, and no painter of devpage's own for one
of the two.

**Why:** the PNG is what an agent checks its work against before a person sees
it. If the PNG and the canvas come from different layouts or different paint code
they drift — a line breaks differently, a shadow goes missing — and the agent
approves a drawing the person never sees. One layout and one display list leave only
the rasterizers' edge anti-aliasing to differ, which no reader can see.

**Check:** the server's PNG route and the WebAssembly module both call the
engine's one function that builds the display list, each backend is a
command-by-command translation, and neither draws anything else. A test paints the example designs with both backends and finds every
pixel within a small anti-aliasing tolerance of the other.
