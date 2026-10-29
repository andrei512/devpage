//! What a display list is made of: the drawing commands both paint
//! backends replay, and the boxes layout gives the nodes they draw.

use kurbo::{Affine, BezPath};
use peniko::{Color as PColor, FontData, Gradient, ImageData};

/// A node's box after layout, in the scene's pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// One glyph of a run: its id in its font and where its origin sits, in the
/// run's coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub id: u32,
    pub x: f32,
    pub y: f32,
}

/// What a `Draw::Fill` covers its path with.
pub enum Paint {
    Solid(PColor),
    /// A gradient whose points are in scene pixels.
    Gradient(Gradient),
    /// An image, placed by the transform from its pixels to scene pixels.
    Image(ImageData, Affine),
}

/// One drawing command, in its chunk's coordinates: what both vello backends
/// replay. Every decision about the drawing is made before a command is
/// made; a backend only translates it into its library's call. The layers it
/// is drawn in are nodes of the property trees, not commands.
pub enum Draw {
    Fill(BezPath, Paint),
    Stroke(BezPath, f32, PColor),
    /// A glyph run: its font, size and color, its point on the font's
    /// variation axes — empty for a static font — and its glyphs.
    Glyphs(FontData, f32, PColor, Vec<i16>, Vec<Glyph>),
}
