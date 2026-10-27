# A design has one renderer

<Callout kind="note" title="The rule">

The canvas in the browser and the PNG a design's address answers come from the
same code: the design engine lays the design out once and paints it into one
display list for the theme it is shown in, which a GPU backend (`vello`) replays on the canvas and a CPU
backend (`vello_cpu`) replays as the PNG, with the same embedded typeface. A
backend only translates commands; it decides nothing about the drawing. Every surface shows the same area: the design's whole drawing, its root's box
plus anything painted outside it. Nothing else draws a design — not
the browser's own layout, SVG or HTML, and no painter of devpage's own for one
of the two.

</Callout>

## Why

The PNG is what an agent checks its work against before a person sees
it. If the PNG and the canvas come from different layouts or different paint code
they drift — a line breaks differently, a shadow goes missing — and the agent
approves a drawing the person never sees. One layout and one display list leave only
the rasterizers' edge anti-aliasing to differ, which no reader can see.

## Check

The server's PNG route and the WebAssembly module both call the
engine's one function that builds the display list, each backend is a
command-by-command translation, and neither draws anything else. Tests
paint the example designs with both backends, in the light theme and in the
dark one, and compare each with the server's PNG in that theme by the
engine's one rule (`render::compare`): a pixel is
**unexplained** when a channel lies more than 8 outside the range of the other
drawing's pixel and its eight neighbours — which is all an anti-aliased edge
can move. At most 0.05% of the pixels may be unexplained, and no cluster of
touching unexplained pixels may be larger than 4. A missing one-pixel divider
or a missing icon fails it; a test proves that on the checkout design.
