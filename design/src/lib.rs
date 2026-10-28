//! The design engine: parses `.design` files, resolves them into scenes, lays
//! them out and paints them. It does no I/O, so the same code runs natively
//! in devpage and compiled to WebAssembly in the browser.

pub mod atlas;
pub mod display;
pub mod fonts;
pub mod parse;
pub mod proto;
pub mod render;
pub mod resolve;
pub mod shadow;
pub mod tiles;
pub mod wire;

use std::fmt;

pub use render::{Damage, Drawing, Renderer};
pub use resolve::{Files, Scene, Theme, resolve};

/// Something wrong with a design, at a place in one of its files.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub file: String,
    pub pos: parse::Pos,
    pub message: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: {}",
            self.file, self.pos.line, self.pos.col, self.message
        )
    }
}

/// Paints a scene's whole drawing at 1× in `theme` and encodes it as a PNG,
/// band by band, or says why it can't: see [`Renderer::drawing`].
pub fn png(renderer: &mut Renderer, scene: &Scene, theme: Theme) -> Result<Vec<u8>, String> {
    let drawing = renderer
        .drawing(scene, theme, None)
        .map_err(|e| e.message)?;
    let mut png = Vec::new();
    tiles::png(&drawing.list, &mut png).map_err(|e| format!("encoding the PNG: {e}"))?;
    Ok(png)
}

// SAFETY: a drawing isn't `Send` only because taffy's `CompactLength`, in
// the layout tree it keeps, packs a length into a `*const ()`. That value is
// a pointer only for a `calc()` length, behind taffy's `calc` feature, which
// the engine neither enables nor makes — every length it gives taffy is a
// number or a keyword, bits in the word — so nothing in a drawing points
// anywhere, and a server can keep one behind a lock and paint it from any
// thread.
unsafe impl Send for render::Drawing {}
