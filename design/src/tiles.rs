//! Tiles: squares of a drawing painted at one scale in one theme, and the PNG
//! assembled band by band from them.

use std::io::Write;

use vello_cpu::kurbo::Rect as KRect;

use crate::display::List;
use crate::render::paint_area;

/// A tile's side, in device pixels.
pub const TILE: u32 = 256;

/// The design-coordinate square of tile `(col, row)` at `scale`, from the
/// drawing's top-left.
pub fn tile_rect(list: &List, scale: f64, col: u32, row: u32) -> KRect {
    let side = TILE as f64 / scale;
    let (x, y) = (list.area.x as f64, list.area.y as f64);
    KRect::new(
        x + col as f64 * side,
        y + row as f64 * side,
        x + (col + 1) as f64 * side,
        y + (row + 1) as f64 * side,
    )
}

/// A tile's pixels, straight RGBA, `TILE` a side.
pub fn tile(list: &List, scale: f64, col: u32, row: u32) -> Vec<u8> {
    let pixmap = paint_area(list, tile_rect(list, scale, col, row), scale);
    let mut rgba = pixmap.data_as_u8_slice().to_vec();
    unpremultiply(&mut rgba);
    rgba
}

/// Straight alpha from premultiplied, rounding as the PNG encoder does.
pub fn unpremultiply(rgba: &mut [u8]) {
    for px in rgba.as_chunks_mut::<4>().0 {
        let a = px[3] as u32;
        if a != 0 && a != 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
}

/// The drawing at 1× as a PNG written to `out`, band by band: each band one
/// row of tiles, its rows streamed to the encoder, so memory holds one band.
pub fn png(list: &List, out: impl Write) -> Result<(), png::EncodingError> {
    let (w, h) = (list.area.width as u32, list.area.height as u32);
    let (w, h) = (w.max(1), h.max(1));
    let mut encoder = png::Encoder::new(out, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut header = encoder.write_header()?;
    let mut writer = header.stream_writer()?;
    let cols = w.div_ceil(TILE);
    for row in 0..h.div_ceil(TILE) {
        let tiles: Vec<Vec<u8>> = (0..cols).map(|col| tile(list, 1.0, col, row)).collect();
        let rows = (h - row * TILE).min(TILE);
        for y in 0..rows {
            for (col, t) in tiles.iter().enumerate() {
                let across = (w - col as u32 * TILE).min(TILE) as usize;
                let start = y as usize * TILE as usize * 4;
                writer.write_all(&t[start..start + across * 4])?;
            }
        }
    }
    writer.finish()
}
