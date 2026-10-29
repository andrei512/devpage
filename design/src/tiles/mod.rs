//! Tiles: squares of a drawing, 256 device pixels a side, painted at one
//! scale in one theme — where each is in the design, the pyramid's levels,
//! and the cache that keeps them and marks the ones damage makes stale.

use kurbo::Rect as KRect;

use crate::display::List;
use crate::scene::Theme;

mod cache;

pub use cache::*;

/// A tile's side, in device pixels.
pub const TILE: u32 = 256;

/// The design-coordinate square of tile `(col, row)` at `scale`, from the
/// drawing's top-left.
pub fn tile_rect(list: &List, scale: f64, col: u32, row: u32) -> KRect {
    square((list.area.x as f64, list.area.y as f64), scale, col, row)
}

/// The design-coordinate square of tile `(col, row)` at `scale` of a drawing
/// whose top-left is `origin`.
pub fn square((x, y): (f64, f64), scale: f64, col: u32, row: u32) -> KRect {
    let side = TILE as f64 / scale;
    KRect::new(
        x + col as f64 * side,
        y + row as f64 * side,
        x + (col + 1) as f64 * side,
        y + (row + 1) as f64 * side,
    )
}

/// A scale tiles are painted at, to a 1/1024th: a level of the pyramid.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Level(pub i64);

impl Level {
    /// The level a scale is painted at.
    pub fn of(scale: f64) -> Level {
        Level((scale * 1024.0).round().max(1.0) as i64)
    }

    pub fn scale(self) -> f64 {
        self.0 as f64 / 1024.0
    }
}

/// A tile: its theme, its level, and its column and row from the drawing's
/// top-left.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Key {
    pub theme: Theme,
    pub level: Level,
    pub col: u32,
    pub row: u32,
}

impl Key {
    /// Its square in the design's coordinates, the drawing's top-left at
    /// `origin`.
    pub fn rect(&self, origin: (f64, f64)) -> KRect {
        square(origin, self.level.scale(), self.col, self.row)
    }
}

/// What a cache keys its tiles by: a tile's [`Key`], with whatever else
/// tells the cache's tiles apart — a server's design, say.
pub trait Tiled: Copy + Eq + std::hash::Hash {
    fn key(&self) -> Key;
}

impl Tiled for Key {
    fn key(&self) -> Key {
        *self
    }
}

/// A view of a drawing: `width` × `height` device pixels showing the
/// drawing scaled by `zoom`, its top-left at `origin`, a whole device pixel.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub width: f64,
    pub height: f64,
    pub zoom: f64,
    pub origin: (f64, f64),
}

impl View {
    /// The tiles at `level` the view shows, as columns and rows of the
    /// drawing `area` × `level`, nearest the view's middle first.
    pub fn tiles(&self, area: KRect, level: Level) -> Vec<(u32, u32)> {
        let side = TILE as f64;
        let scale = level.scale();
        let (ox, oy) = self.origin;
        let count = |v: f64| (v * scale / side).ceil() as i64;
        let (cols, rows) = (count(area.width()), count(area.height()));
        let span = |o: f64, n: f64, count: i64| {
            let first = ((-o) / side).floor().max(0.0) as i64;
            let last = (((n - o) / side).ceil() as i64).min(count);
            first..last.max(first)
        };
        let (cs, rs) = (span(ox, self.width, cols), span(oy, self.height, rows));
        let mut out: Vec<(u32, u32)> = rs
            .flat_map(|r| cs.clone().map(move |c| (c as u32, r as u32)))
            .collect();
        let (mx, my) = (
            (self.width / 2.0 - ox) / side,
            (self.height / 2.0 - oy) / side,
        );
        let d = |p: &(u32, u32)| (p.0 as f64 + 0.5 - mx).powi(2) + (p.1 as f64 + 0.5 - my).powi(2);
        out.sort_by(|a, b| d(a).total_cmp(&d(b)));
        out
    }

    /// Where a tile at any level lands in the view, in device pixels —
    /// `(x0, y0, x1, y1)` — or `None` when the view doesn't show it.
    pub fn place(&self, area: KRect, key: &Key) -> Option<KRect> {
        let rect = square((area.x0, area.y0), key.level.scale(), key.col, key.row);
        let (ox, oy) = self.origin;
        let to = |x: f64, y: f64| {
            (
                ox + (x - area.x0) * self.zoom,
                oy + (y - area.y0) * self.zoom,
            )
        };
        let (x0, y0) = to(rect.x0, rect.y0);
        let (x1, y1) = to(rect.x1, rect.y1);
        (x1 >= 0.0 && y1 >= 0.0 && x0 <= self.width && y0 <= self.height)
            .then(|| KRect::new(x0, y0, x1, y1))
    }
}

#[cfg(test)]
mod tests;
