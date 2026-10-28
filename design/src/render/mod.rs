//! Scene → pixels. The engine lays a scene out ([`layout`]: taffy, parley),
//! paints each node's own commands ([`painter`]) into a [`Drawing`]
//! ([`drawing`]): its display list in chunks under property trees, kept and
//! brought up to date by what changes ([`update`]). A backend paints any area
//! of it through [`crate::display::paint`] — `vello_cpu` here ([`cpu`]), for
//! the PNG and tiles; [`compare`] says whether two drawings agree.
//! This module holds the renderer — its fonts and text shaping — and the
//! commands' types.

use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontWeight, Layout as Text,
    LayoutContext, LineHeight, StyleProperty,
};
use std::collections::{BTreeSet, HashMap};
use taffy::prelude::*;

use vello_cpu::Glyph;
use vello_cpu::kurbo::{Affine, BezPath, Rect as KRect};
use vello_cpu::peniko::{Color as PColor, FontData, Gradient, ImageData};

use crate::fonts::{self, FontFiles, FontId};
use crate::resolve::{self, ImageId, Origin, TextAlign};

mod compare;
mod cpu;
mod drawing;
#[cfg(test)]
mod examples;
mod layout;
mod painter;
#[cfg(test)]
pub(crate) mod testing;
mod update;

pub use compare::{CLUSTER, Difference, EDGE, SHARE, compare, matches, pixels};
pub use cpu::{Cpu, paint_area};
use drawing::*;
pub use drawing::{Damage, Drawing};
use layout::*;
use painter::*;

const FONTS: [&[u8]; 4] = [
    include_bytes!("../../fonts/Inter-Regular.ttf"),
    include_bytes!("../../fonts/Inter-Medium.ttf"),
    include_bytes!("../../fonts/Inter-SemiBold.ttf"),
    include_bytes!("../../fonts/Inter-Bold.ttf"),
];

/// The fonts and text layout state a renderer keeps between paints.
pub struct Renderer {
    fonts: FontContext,
    /// The embedded Inter faces, each under one blob for the renderer's life.
    inter: Vec<parley::fontique::Blob<u8>>,
    /// The font files registered besides Inter — those of a scene laid out —
    /// or `None` before anything is.
    registered: Option<BTreeSet<FontId>>,
    layouts: LayoutContext<()>,
    /// While a display list is made: the image each image file is painted
    /// from in it, and every image it paints, in the order it first does,
    /// with the attribute that asked for it.
    blobs: HashMap<ImageId, ImageData>,
    painted: Vec<(Painted, Origin)>,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

/// A node's box after layout, in the scene's pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
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

/// Why a scene can't be drawn: images and shadows that don't fit the GPU's
/// atlas — `at` the attribute that broke it, when one did.
#[derive(Debug, Clone, PartialEq)]
pub struct Undrawable {
    pub message: String,
    pub at: Option<Origin>,
}

impl std::fmt::Display for Undrawable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl Renderer {
    pub fn new() -> Renderer {
        let fonts = FontContext {
            collection: parley::fontique::Collection::new(parley::fontique::CollectionOptions {
                shared: false,
                system_fonts: false,
            }),
            source_cache: parley::fontique::SourceCache::default(),
        };
        let mut renderer = Renderer {
            fonts,
            inter: FONTS
                .iter()
                .map(|font| parley::fontique::Blob::new(std::sync::Arc::new(*font)))
                .collect(),
            registered: None,
            layouts: LayoutContext::new(),
            blobs: HashMap::new(),
            painted: Vec::new(),
        };
        renderer.register(&FontFiles::new());
        renderer
    }

    /// Makes the scene's font files available to layout, each under a
    /// family name of its own. The collection keeps what it has while a
    /// scene uses no other font file; otherwise it starts again from Inter
    /// and this scene's, so it never holds more than one scene's fonts.
    fn register(&mut self, files: &FontFiles) {
        if let Some(registered) = &self.registered
            && files.keys().all(|id| registered.contains(id))
        {
            return;
        }
        let collection = &mut self.fonts.collection;
        collection.clear();
        for blob in &self.inter {
            collection.register_fonts(blob.clone(), None);
        }
        for (id, file) in files {
            let family = id.family();
            collection.register_fonts(
                file.blob.clone(),
                Some(parley::fontique::FontInfoOverride {
                    family_name: Some(&family),
                    ..Default::default()
                }),
            );
        }
        self.registered = Some(files.keys().copied().collect());
    }

    fn text_layout(&mut self, text: &resolve::TextStyle, width: Option<f32>) -> Text<()> {
        let mut builder = self
            .layouts
            .ranged_builder(&mut self.fonts, &text.content, 1.0, true);
        let family = match text.font {
            Some(id) => id.family(),
            None => fonts::INTER.to_string(),
        };
        builder.push_default(StyleProperty::FontFamily(FontFamily::named(&family)));
        builder.push_default(StyleProperty::FontSize(text.size));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(
            text.weight as f32,
        )));
        builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
            text.line_height,
        )));
        let mut layout = builder.build(&text.content);
        layout.break_all_lines(width);
        let align = match text.align {
            TextAlign::Left => Alignment::Left,
            TextAlign::Center => Alignment::Center,
            TextAlign::Right => Alignment::Right,
        };
        layout.align(align, AlignmentOptions::default());
        layout
    }
}

fn frame_rect(f: Frame) -> KRect {
    KRect::new(
        f.x as f64,
        f.y as f64,
        (f.x + f.width) as f64,
        (f.y + f.height) as f64,
    )
}
