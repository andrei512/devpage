//! The design engine: parses `.design` files, resolves them into scenes, lays
//! them out and paints them. It does no I/O, so the same code runs natively
//! in devpage and compiled to WebAssembly in the browser.
//!
//! **The core** links no rasterizer: text → syntax tree ([`parse`]) → scene
//! ([`resolve`], [`fonts`]; what a scene is, and the edits from one to the
//! next, are [`scene`]'s) → layout, the painter's commands and a kept,
//! updatable drawing ([`render`]) → chunks under property trees with their
//! index, the one paint entry point and hit testing ([`display`]) → tiles,
//! their cache and the damage that makes them stale ([`tiles`]) — and a
//! design as it is held while shown, its scene and its drawings brought
//! along by each edit, with one rule for a scene that can't be drawn
//! ([`design`]); with the
//! shadows' sampling ([`shadow`]), the GPU atlas's model ([`atlas`]), the
//! wire form of scenes and their edits ([`wire`], [`proto`]) and the rule two
//! drawings are compared by ([`compare`]). Its geometry and colours are
//! kurbo's and peniko's, depended on directly.
//!
//! **Adapters** plug into its one port, [`display::Target`] — paint this area
//! at this scale: [`cpu`], `vello_cpu`; the module's `vello` on WebGPU. And
//! [`bands`] writes the PNG band by band from whatever gives it tiles. The
//! core names no adapter; a test holds it to that.

// The core.
pub mod atlas;
pub mod compare;
pub mod design;
pub mod display;
pub mod fonts;
pub mod ids;
pub mod parse;
pub mod proto;
pub mod render;
pub mod resolve;
pub mod scene;
pub mod shadow;
pub mod sums;
pub mod tiles;
pub mod tree;
pub mod wire;

// Adapters.
pub mod bands;
pub mod cpu;

use std::fmt;

pub use design::Design;
pub use kurbo::Rect as KRect;
pub use render::{Damage, Drawing, Renderer};
pub use resolve::resolve;
pub use scene::{Files, Scene, Theme};

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
/// band by band, or says why it can't: see [`Renderer::drawing`]. A drawing
/// made afresh and kept by nothing: the server's PNG comes from its kept
/// drawings and tiles instead. This is the reference the engine's, the
/// module's and the canvas's tests compare a drawing against — one call,
/// from a scene, with nothing kept.
pub fn png(renderer: &mut Renderer, scene: &Scene, theme: Theme) -> Result<Vec<u8>, String> {
    let drawing = renderer
        .drawing(scene, theme, None)
        .map_err(|e| e.message)?;
    let mut png = Vec::new();
    bands::png(&drawing.list, &mut png).map_err(|e| format!("encoding the PNG: {e}"))?;
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

/// The core names no adapter and links no rasterizer: no module of it
/// mentions `vello_cpu`, `vello`, `wgpu`, the PNG encoder, or [`cpu`] and
/// [`bands`] — outside its tests. Dependencies point inward.
#[cfg(test)]
#[test]
fn the_core_names_no_adapter() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let adapters = ["lib.rs", "cpu", "bands"];
    let forbidden = [
        "vello_cpu",
        "vello::",
        "wgpu",
        "png::Encoder",
        "png::Stream",
        "crate::cpu",
        "crate::bands",
    ];
    let mut stack = vec![src.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let rel = path.strip_prefix(&src).unwrap();
            let top = rel
                .components()
                .next()
                .unwrap()
                .as_os_str()
                .to_string_lossy();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if adapters.contains(&top.trim_end_matches(".rs"))
                || adapters.contains(&top.as_ref())
                || name == "tests.rs"
                || name == "testing.rs"
                || name == "examples.rs"
            {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            // A module's own unit tests sit at its end.
            let text = text.split("#[cfg(test)]\nmod tests {").next().unwrap();
            for word in forbidden {
                assert!(!text.contains(word), "{} names {word}", rel.display());
            }
        }
    }
}
