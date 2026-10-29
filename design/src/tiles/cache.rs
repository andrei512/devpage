//! The tile cache: tiles by key, capped by a cost, least recently used
//! dropped first; damage marks the tiles it overlaps stale, and a stale tile
//! is still returned, as stale, until it is replaced. Generic over what a
//! tile holds — the server's pixels, the module's GPU textures.

use std::collections::{BTreeMap, HashMap};

use kurbo::Rect as KRect;

use crate::scene::Theme;

use super::*;

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
mod tests;
