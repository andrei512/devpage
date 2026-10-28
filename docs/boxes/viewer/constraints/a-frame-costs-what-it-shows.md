# A frame costs what it shows

<Callout kind="note" title="The rule">

A design has no size limit. Painting a view of a design costs what the view
shows, and applying an edit costs what the edit changed — the nodes whose
own box or look it changes, plus log time in each frame on their way to the
root — never the size of the whole design, nor the number of siblings beside
what changed. The one
step that stays linear is reading the file that was saved: parsing it and
matching its elements against the last parse. Nothing that paints a frame, a tile or a PNG band, applies
a patch or answers a click walks every node or every command; it asks the
[index](../design-engine/README.md#the-index) what touches the area. A pan or
a zoom repaints nothing: it moves and scales tiles already painted.

</Callout>

## Why

A design grows with what it describes — a long flow, a page of screens, a
whole app's components on one canvas. If any step costs the whole design, it
gets a ceiling: a size past which the PNG an agent checks its work against
can't be painted, or a canvas that stutters on every pan and blurs on every
zoom, or a save that repaints everything to change one word. The agent and the
reader would then see less than the design says, exactly where it matters most.

## Check

Review, on every change to the engine, the canvas or the PNG route: a path
that loops over all of a drawing's nodes, chunks or commands outside making a
drawing from a new scene is the finding. Tests pin it on a design 100,000
pixels tall: the canvas paints only the tiles of its view; a pan's cost doesn't
grow with the design; the GPU and the CPU agree on a zoomed-in view; its PNG
is made holding one band; and an edit to one node repaints only the tiles
around it, in the browser and in the server. An edit to one item of a list,
and an item inserted before its first, cost the same at 1,000, 10,000 and
100,000 siblings, from the save to the patch to the drawing; and a drawing
1,000,000 pixels wide has its PNG made within the band's bytes.
