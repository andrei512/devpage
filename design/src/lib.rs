//! The design engine: parses `.design` files, resolves them into scenes, lays
//! them out and paints them. It does no I/O, so the same code runs natively
//! in devpage and compiled to WebAssembly in the browser.

pub mod parse;
pub mod proto;
pub mod render;
pub mod resolve;
pub mod wire;

use std::fmt;

pub use render::Renderer;
pub use resolve::{Files, Scene, resolve};

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

/// Paints a scene's whole drawing at 1× and encodes it as a PNG, or says why
/// it can't: a drawing bigger than `render::MAX_SIDE` a side.
pub fn png(renderer: &mut Renderer, scene: &Scene) -> Result<Vec<u8>, String> {
    let drawing = renderer.drawing(scene)?;
    render::paint_list(drawing.area, &drawing.list, 1.0)
        .into_png()
        .map_err(|e| format!("encoding the PNG: {e}"))
}
