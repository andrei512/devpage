//! The PNG, written band by band: each band as many rows as fit in
//! [`BAND_BYTES`] of pixels, its rows streamed into the `png` crate's encoder
//! as it is had, so what is held at once is one band — whatever paints or
//! caches its pieces, and however wide the drawing is.

use std::io::Write;

use crate::display::List;
use crate::render::Renderer;
use crate::scene::{Scene, Theme};
use crate::tiles::TILE;

/// The most pixel bytes a band holds: a band is as many rows as fit, at least
/// one. A drawing up to 16,384 pixels wide fits a whole row of tiles.
pub const BAND_BYTES: usize = 16 << 20;

/// A piece of a band, `TILE` pixels wide at column `col`: a whole tile, at
/// tile row `row`, or a strip `rows` tall starting at pixel row `y` — what a
/// band of a drawing too wide for a row of tiles is made of. Either is its
/// straight RGBA pixels, `TILE` wide, row after row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Piece {
    Tile { col: u32, row: u32 },
    Strip { col: u32, y: u32, rows: u32 },
}

/// How many rows a band of a drawing `width` pixels wide holds: as many as
/// fit in [`BAND_BYTES`], its pieces padded to whole tiles, at least one and
/// at most a tile's.
pub fn rows_per_band(width: u32) -> u32 {
    let padded = width.max(1).div_ceil(TILE) as usize * TILE as usize * 4;
    (BAND_BYTES / padded).clamp(1, TILE as usize) as u32
}

/// A scene's whole drawing, painted at 1× in `theme` and encoded as a PNG,
/// band by band, or why it can't be: see [`Renderer::drawing`]. A drawing
/// made afresh and kept by nothing: the server's PNG comes from its kept
/// drawings and tiles instead. This is the reference the engine's, the
/// module's and the canvas's tests compare a drawing against — one call,
/// from a scene, with nothing kept.
pub fn fresh(renderer: &mut Renderer, scene: &Scene, theme: Theme) -> Result<Vec<u8>, String> {
    let drawing = renderer
        .drawing(scene, theme, None)
        .map_err(|e| e.message)?;
    let mut png = Vec::new();
    write(&drawing.list, &mut png).map_err(|e| format!("encoding the PNG: {e}"))?;
    Ok(png)
}

/// The drawing at 1× as a PNG written to `out`, band by band.
pub fn write(list: &List, out: impl Write) -> Result<(), png::EncodingError> {
    let (w, h) = (list.area.width as u32, list.area.height as u32);
    write_bands(w, h, out, |piece| {
        Ok::<_, std::convert::Infallible>(match piece {
            Piece::Tile { col, row } => crate::cpu::tile(list, 1.0, col, row),
            Piece::Strip { col, y, rows } => crate::cpu::strip(list, col, y, rows),
        })
    })
    .map_err(|e| match e {
        Band::Encoding(e) => e,
    })
}

/// Why writing a PNG from bands stopped: the encoder or its writer failed,
/// or a piece couldn't be had.
#[derive(Debug)]
pub enum Band<E> {
    Encoding(png::EncodingError),
    Tile(E),
}

impl<E: std::fmt::Display> std::fmt::Display for Band<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Band::Encoding(e) => write!(f, "{e}"),
            Band::Tile(e) => write!(f, "{e}"),
        }
    }
}

/// A `width` × `height` PNG written to `out` band by band, each band's pieces
/// from `piece` — whole tiles while a row of them fits in [`BAND_BYTES`],
/// shorter strips otherwise — its rows streamed to the encoder as each band
/// is had, so what is held at once is one band.
pub fn write_bands<P: AsRef<[u8]>, E>(
    width: u32,
    height: u32,
    out: impl Write,
    mut piece: impl FnMut(Piece) -> Result<P, E>,
) -> Result<(), Band<E>> {
    let (w, h) = (width.max(1), height.max(1));
    let mut encoder = png::Encoder::new(out, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut header = encoder.write_header().map_err(Band::Encoding)?;
    let mut writer = header.stream_writer().map_err(Band::Encoding)?;
    let cols = w.div_ceil(TILE);
    let per = rows_per_band(w);
    let mut y = 0;
    while y < h {
        let rows = per.min(h - y);
        let band = (0..cols)
            .map(|col| {
                piece(if per == TILE {
                    Piece::Tile { col, row: y / TILE }
                } else {
                    Piece::Strip { col, y, rows }
                })
            })
            .collect::<Result<Vec<P>, E>>()
            .map_err(Band::Tile)?;
        for r in 0..rows {
            for (col, p) in band.iter().enumerate() {
                let across = (w - col as u32 * TILE).min(TILE) as usize;
                let start = r as usize * TILE as usize * 4;
                writer
                    .write_all(&p.as_ref()[start..start + across * 4])
                    .map_err(|e| Band::Encoding(e.into()))?;
            }
        }
        y += rows;
    }
    writer.finish().map_err(Band::Encoding)
}

#[cfg(test)]
mod tests;
