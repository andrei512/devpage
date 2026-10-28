//! The PNG, written band by band: each band one row of tiles, its rows
//! streamed into the `png` crate's encoder as it is had, so what is held at
//! once is one band — whatever paints or caches the tiles.

use std::io::Write;

use crate::display::List;
use crate::tiles::TILE;

/// The drawing at 1× as a PNG written to `out`, band by band: each band one
/// row of tiles, its rows streamed to the encoder, so memory holds one band.
pub fn png(list: &List, out: impl Write) -> Result<(), png::EncodingError> {
    let (w, h) = (list.area.width as u32, list.area.height as u32);
    bands(w, h, out, |col, row| {
        Ok::<_, std::convert::Infallible>(crate::cpu::tile(list, 1.0, col, row))
    })
    .map_err(|e| match e {
        Band::Encoding(e) => e,
    })
}

/// Why writing a PNG from bands stopped: the encoder or its writer failed,
/// or a tile couldn't be had.
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

/// A `width` × `height` PNG written to `out` band by band — one row of tiles
/// at a time, each tile's straight RGBA pixels, `TILE` a side, from `tile`
/// — its rows streamed to the encoder as each band is had, so what is held
/// at once is one band of tiles.
pub fn bands<P: AsRef<[u8]>, E>(
    width: u32,
    height: u32,
    out: impl Write,
    mut tile: impl FnMut(u32, u32) -> Result<P, E>,
) -> Result<(), Band<E>> {
    let (w, h) = (width.max(1), height.max(1));
    let mut encoder = png::Encoder::new(out, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut header = encoder.write_header().map_err(Band::Encoding)?;
    let mut writer = header.stream_writer().map_err(Band::Encoding)?;
    let cols = w.div_ceil(TILE);
    for row in 0..h.div_ceil(TILE) {
        let band = (0..cols)
            .map(|col| tile(col, row))
            .collect::<Result<Vec<P>, E>>()
            .map_err(Band::Tile)?;
        let rows = (h - row * TILE).min(TILE);
        for y in 0..rows {
            for (col, t) in band.iter().enumerate() {
                let across = (w - col as u32 * TILE).min(TILE) as usize;
                let start = y as usize * TILE as usize * 4;
                writer
                    .write_all(&t.as_ref()[start..start + across * 4])
                    .map_err(|e| Band::Encoding(e.into()))?;
            }
        }
    }
    writer.finish().map_err(Band::Encoding)
}

#[cfg(test)]
mod tests;
