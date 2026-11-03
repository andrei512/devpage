//! What a display list is made of: the drawing commands both paint
//! backends replay, and the boxes layout gives the nodes they draw.

use kurbo::{Affine, BezPath};
use peniko::{Color as PColor, FontData, Gradient, ImageData};

/// A node's box after layout, in the scene's pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// Where its left edge is.
    pub x: f32,
    /// Where its top edge is.
    pub y: f32,
    /// How far it reaches right of `x`.
    pub width: f32,
    /// How far it reaches down from `y`.
    pub height: f32,
}

/// One glyph of a run: its id in its font and where its origin sits, in the
/// run's coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    /// The glyph's id in its font.
    pub id: u32,
    /// Where its origin is across.
    pub x: f32,
    /// Where its origin is down: on the line's baseline.
    pub y: f32,
}

/// What a `Draw::Fill` covers its path with.
pub enum Paint {
    /// One color, all over.
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
    /// A path filled with a paint.
    Fill(BezPath, Paint),
    /// A path's outline, stroked in one color, as wide in pixels as the `f32`
    /// says.
    Stroke(BezPath, f32, PColor),
    /// A glyph run: its font, size and color, its point on the font's
    /// variation axes — empty for a static font — and its glyphs.
    Glyphs(FontData, f32, PColor, Vec<i16>, Vec<Glyph>),
}
