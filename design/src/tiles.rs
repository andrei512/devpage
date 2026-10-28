//! Tiles: squares of a drawing painted at one scale in one theme, and the PNG
//! assembled band by band from them.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;

use vello_cpu::kurbo::Rect as KRect;

use crate::resolve::Theme;

use crate::display::List;
use crate::render::paint_area;

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
    bands(w, h, out, |col, row| {
        Ok::<_, std::convert::Infallible>(tile(list, 1.0, col, row))
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

/// A cached tile: what it holds, what it costs, whether damage made it
/// stale, and when it was last used.
struct Entry<T> {
    value: T,
    cost: usize,
    stale: bool,
    used: u64,
}

/// Painted tiles, keyed by theme, level, column and row, holding at most
/// `cap` of cost — bytes of pixels, or a count — the least recently used
/// dropped first. Damage marks the tiles it overlaps stale; a stale tile is
/// still there, and says so, until it is painted again. What a tile holds is
/// the cache's user's: pixels in the server, a texture on the GPU.
pub struct Cache<T, K: Tiled = Key> {
    cap: usize,
    cost: usize,
    clock: u64,
    entries: HashMap<K, Entry<T>>,
    by_use: BTreeMap<u64, K>,
}

impl<T, K: Tiled> Cache<T, K> {
    pub fn new(cap: usize) -> Cache<T, K> {
        Cache {
            cap,
            cost: 0,
            clock: 0,
            entries: HashMap::new(),
            by_use: BTreeMap::new(),
        }
    }

    /// The tile at `key`, and whether it is stale; it counts as used.
    pub fn get(&mut self, key: &K) -> Option<(&T, bool)> {
        self.clock += 1;
        let e = self.entries.get_mut(key)?;
        self.by_use.remove(&e.used);
        e.used = self.clock;
        self.by_use.insert(self.clock, *key);
        Some((&e.value, e.stale))
    }

    /// The tile at `key` and whether it is stale, without counting a use.
    pub fn peek(&self, key: &K) -> Option<(&T, bool)> {
        self.entries.get(key).map(|e| (&e.value, e.stale))
    }

    /// Keeps `value` as the current tile at `key`, costing `cost`, and drops
    /// the least recently used others until the cache is within its cap.
    /// Returns what was dropped.
    pub fn insert(&mut self, key: K, value: T, cost: usize) -> Vec<(K, T)> {
        // The tile it replaces isn't dropped: it is the same tile, painted again.
        self.remove(&key);
        let mut dropped = Vec::new();
        self.clock += 1;
        self.entries.insert(
            key,
            Entry {
                value,
                cost,
                stale: false,
                used: self.clock,
            },
        );
        self.by_use.insert(self.clock, key);
        self.cost += cost;
        while self.cost > self.cap {
            let Some((_, oldest)) = self.by_use.iter().next().map(|(u, k)| (*u, *k)) else {
                break;
            };
            if oldest == key {
                break;
            }
            if let Some(v) = self.remove(&oldest) {
                dropped.push((oldest, v));
            }
        }
        dropped
    }

    /// Takes the tile at `key` out.
    pub fn remove(&mut self, key: &K) -> Option<T> {
        let e = self.entries.remove(key)?;
        self.by_use.remove(&e.used);
        self.cost -= e.cost;
        Some(e.value)
    }

    /// Marks stale every tile of `theme` that overlaps one of `rects` — in
    /// the design's coordinates, the drawing's top-left at `origin` — at
    /// every level.
    pub fn damage(&mut self, theme: Theme, origin: (f64, f64), rects: &[KRect]) {
        self.damage_where(|k| k.key().theme == theme, origin, rects);
    }

    /// Marks stale every tile `which` picks that overlaps one of `rects`, as
    /// [`Cache::damage`] does.
    pub fn damage_where(
        &mut self,
        which: impl Fn(&K) -> bool,
        origin: (f64, f64),
        rects: &[KRect],
    ) {
        for (k, e) in &mut self.entries {
            if e.stale || !which(k) {
                continue;
            }
            let r = k.key().rect(origin);
            if rects
                .iter()
                .any(|d| d.x0 < r.x1 && d.x1 > r.x0 && d.y0 < r.y1 && d.y1 > r.y0)
            {
                e.stale = true;
            }
        }
    }

    /// Marks every tile of `theme` stale.
    pub fn damage_all(&mut self, theme: Theme) {
        self.damage_all_where(|k| k.key().theme == theme);
    }

    /// Marks every tile `which` picks stale.
    pub fn damage_all_where(&mut self, which: impl Fn(&K) -> bool) {
        for (k, e) in &mut self.entries {
            if which(k) {
                e.stale = true;
            }
        }
    }

    /// Drops every tile `which` picks.
    pub fn forget(&mut self, which: impl Fn(&K) -> bool) {
        let gone: Vec<K> = self.entries.keys().filter(|k| which(k)).copied().collect();
        for k in gone {
            self.remove(&k);
        }
    }

    /// Every tile, with whether it is stale.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &T, bool)> {
        self.entries.iter().map(|(k, e)| (k, &e.value, e.stale))
    }

    /// Drops every tile.
    pub fn clear(&mut self) -> Vec<T> {
        self.by_use.clear();
        self.cost = 0;
        self.entries.drain().map(|(_, e)| e.value).collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// What the tiles held cost together.
    pub fn cost(&self) -> usize {
        self.cost
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(level: f64, col: u32, row: u32) -> Key {
        Key {
            theme: Theme::Light,
            level: Level::of(level),
            col,
            row,
        }
    }

    /// The band writer holds one band of tiles at a time: every tile of a
    /// band is let go before the next band's first is asked for, and the
    /// rows it writes are the tiles' rows, in order.
    #[test]
    fn bands_hold_one_row_of_tiles_at_a_time() {
        use std::cell::Cell;
        use std::rc::Rc;
        struct Held(Vec<u8>, Rc<Cell<usize>>);
        impl AsRef<[u8]> for Held {
            fn as_ref(&self) -> &[u8] {
                &self.0
            }
        }
        impl Drop for Held {
            fn drop(&mut self) {
                self.1.set(self.1.get() - 1);
            }
        }
        let (alive, most) = (Rc::new(Cell::new(0)), Cell::new(0));
        let (w, h) = (600u32, 1000u32);
        let mut png = Vec::new();
        bands(w, h, &mut png, |col, row| {
            alive.set(alive.get() + 1);
            most.set(most.get().max(alive.get()));
            // Each pixel says which tile it is in.
            let px = [col as u8, row as u8, 0, 255];
            Ok::<_, ()>(Held(px.repeat((TILE * TILE) as usize), alive.clone()))
        })
        .unwrap();
        assert_eq!((alive.get(), most.get()), (0, 3));
        let image = crate::resolve::decode(&png).unwrap();
        assert_eq!((image.width, image.height), (w, h));
        let at = |x: u32, y: u32| {
            let i = ((y * w + x) * 4) as usize;
            image.data.data()[i..i + 2].to_vec()
        };
        assert_eq!(at(599, 999), vec![2, 3]);
        assert_eq!(at(256, 255), vec![1, 0]);
        assert_eq!(at(255, 256), vec![0, 1]);
        // A tile that can't be had stops the PNG there.
        let stopped = bands(w, h, Vec::new(), |_, row| {
            if row == 1 {
                Err("no tile")
            } else {
                Ok(vec![0u8; (TILE * TILE * 4) as usize])
            }
        });
        assert!(matches!(stopped, Err(Band::Tile("no tile"))));
    }

    #[test]
    fn the_cache_holds_its_cap_and_drops_the_least_recently_used_first() {
        let mut c = Cache::new(3);
        for col in 0..3 {
            assert!(c.insert(key(1.0, col, 0), col, 1).is_empty());
        }
        // Using the first makes the second the oldest.
        assert_eq!(c.get(&key(1.0, 0, 0)), Some((&0, false)));
        let dropped = c.insert(key(1.0, 3, 0), 3, 1);
        assert_eq!(dropped, vec![(key(1.0, 1, 0), 1)]);
        assert_eq!((c.len(), c.cost()), (3, 3));
        // Replacing a tile costs nothing more.
        assert!(c.insert(key(1.0, 3, 0), 30, 1).is_empty());
        assert_eq!(c.peek(&key(1.0, 3, 0)), Some((&30, false)));
        // A tile costing more than the rest drops as many as it takes.
        let dropped = c.insert(key(1.0, 9, 9), 9, 3);
        assert_eq!(dropped.len(), 3);
        assert_eq!((c.len(), c.cost()), (1, 3));
    }

    #[test]
    fn damage_marks_exactly_the_tiles_it_overlaps_stale_at_every_level() {
        let mut c = Cache::new(usize::MAX);
        for level in [0.5, 1.0, 2.0] {
            for col in 0..4 {
                for row in 0..4 {
                    c.insert(key(level, col, row), (), 1);
                }
            }
        }
        let mut dark = key(1.0, 0, 0);
        dark.theme = Theme::Dark;
        c.insert(dark, (), 1);
        // The drawing's top-left at (-10, 20); a rect inside tile (1, 0) at
        // 1×, in tile (0, 0) at ½×, across tiles (2..=3, 0..=1) at 2×.
        let origin = (-10.0, 20.0);
        let damage = [KRect::new(250.0, 30.0, 300.0, 200.0)];
        c.damage(Theme::Light, origin, &damage);
        let mut stale: Vec<(i64, u32, u32)> = c
            .iter()
            .filter(|(_, _, s)| *s)
            .map(|(k, _, _)| (k.level.0, k.col, k.row))
            .collect();
        stale.sort();
        assert_eq!(
            stale,
            vec![(512, 0, 0), (1024, 1, 0), (2048, 2, 0), (2048, 2, 1)]
        );
        // A stale tile is still returned, as stale, until replaced.
        assert_eq!(c.get(&key(1.0, 1, 0)), Some((&(), true)));
        c.insert(key(1.0, 1, 0), (), 1);
        assert_eq!(c.get(&key(1.0, 1, 0)), Some((&(), false)));
        assert_eq!(c.peek(&dark), Some((&(), false)));
        // An edge that only touches a tile doesn't make it stale.
        c.damage(
            Theme::Light,
            origin,
            &[KRect::new(246.0, 30.0, 246.0, 40.0)],
        );
        assert_eq!(c.peek(&key(1.0, 0, 0)), Some((&(), false)));
    }
}
