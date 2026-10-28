//! The display list: drawing commands in chunks — one per scene node, in the
//! node's own coordinates — under property trees, and the two-level index
//! over them. A command doesn't nest layers; it names its clip and its
//! effect, and [`paint`] opens and closes exactly the layers each command it
//! paints needs, so any subset of the list paints in order. Every tile, PNG
//! band and click goes through here.

mod chunk;
mod hit;
mod list;
mod paint;
mod trees;

pub use chunk::*;
pub use list::*;
pub use paint::*;
pub use trees::*;
