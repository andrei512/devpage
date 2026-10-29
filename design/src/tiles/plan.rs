//! The frame plan: what a browser canvas shows each frame and which tiles
//! it paints next, for whichever host carries it out — the GPU, painting
//! each tile into a texture as it is asked, or the page on the CPU, asking
//! a worker and keeping its bitmaps. The plan holds all of the state that
//! decides it: the tiles held, least recently used dropped first past a
//! cap, which damage made stale; the tiles asked for and not yet arrived,
//! each under a ticket, and whether damage reached it meanwhile. A host
//! keeps only the paintings, by ticket.
//!
//! **One budget rule**: at most a budget of tiles are in flight — asked for
//! and not yet arrived. The GPU paints what it is asked for at once, so its
//! budget is the tiles of a frame; the CPU's worker answers later, so a
//! frame asks only for as many as have arrived since.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use kurbo::Rect as KRect;

use crate::scene::Theme;

use super::*;

/// What a host knows a painting by: given when a tile is asked for, never
/// given again by any plan, so an answer for the design a canvas showed
/// before matches nothing.
pub type Ticket = u32;

/// The most tiles a canvas holds: more than a 4K screen at a device pixel
/// ratio of 2 shows, about 160, so a view never evicts its own.
pub const CAP: usize = 384;

/// The most tiles a canvas has in flight at once.
pub const BUDGET: usize = 6;

/// The next ticket; 0 is never given.
static NEXT: AtomicU32 = AtomicU32::new(1);

/// A tile asked for and not yet arrived.
struct Asked {
    key: Key,
    /// Damage reached it after it was asked for: what arrives is stale.
    stale: bool,
}

/// One canvas's frame plan.
pub struct Plan {
    /// The tiles held, each by the ticket its painting arrived under.
    held: Cache<Ticket>,
    /// The tiles in flight, by ticket.
    asked: HashMap<Ticket, Asked>,
    budget: usize,
    /// Paintings let go since the last frame.
    evicted: Vec<Ticket>,
}

/// A frame's work: the paintings to draw, bottom first, each where it lands in
/// the view, in device pixels; the tiles to paint next, each under its
/// ticket; the paintings to free; and how many tiles of the view at its own
/// level are missing or stale — 0 when the frame shows the view current.
#[derive(Debug)]
pub struct Work {
    pub draw: Vec<(Ticket, KRect)>,
    pub request: Vec<(Ticket, Key)>,
    pub evict: Vec<Ticket>,
    pub missing: usize,
}

/// What became of a painting that arrived.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arrival {
    /// Its ticket isn't one in flight: the host frees it.
    Dropped,
    /// Held, current.
    Current,
    /// Held and shown, but damage reached its tile while it was painted: it
    /// is asked for again.
    Stale,
}

impl Plan {
    /// A plan holding at most `cap` tiles, with at most `budget` in flight.
    pub fn new(cap: usize, budget: usize) -> Plan {
        Plan {
            held: Cache::new(cap),
            asked: HashMap::new(),
            budget,
            evicted: Vec::new(),
        }
    }

    /// Marks stale every tile of `theme`, held or in flight, that overlaps
    /// one of `rects` — in the design's coordinates, the drawing's top-left
    /// at `origin` — at every level.
    pub fn damage(&mut self, theme: Theme, origin: (f64, f64), rects: &[KRect]) {
        self.held.damage(theme, origin, rects);
        for asked in self.asked.values_mut() {
            if asked.key.theme == theme && asked.key.overlaps(origin, rects) {
                asked.stale = true;
            }
        }
    }

    /// Marks every tile of `theme`, held or in flight, stale.
    pub fn damage_all(&mut self, theme: Theme) {
        self.held.damage_all(theme);
        for asked in self.asked.values_mut() {
            if asked.key.theme == theme {
                asked.stale = true;
            }
        }
    }

    /// The frame of `view` of the drawing `area` in `theme`, which `needed`
    /// — the tiles the view needs at its own level, nearest its middle first
    /// — makes current. Every held tile of the theme the view shows is
    /// drawn, as a stand-in where it is stale or at another level; once a
    /// zoom has `settled`, the missing and stale tiles are asked for in
    /// `needed`'s order while fewer than the budget are in flight.
    pub fn frame(
        &mut self,
        theme: Theme,
        area: KRect,
        view: &View,
        needed: &[(u32, u32)],
        settled: bool,
    ) -> Work {
        let level = Level::of(view.zoom);
        let missing: Vec<Key> = needed
            .iter()
            .map(|&(col, row)| Key {
                theme,
                level,
                col,
                row,
            })
            // Each tile the view needs counts as used.
            .filter(|k| self.held.get(k).is_none_or(|(_, stale)| stale))
            .collect();
        let mut draw: Vec<(Level, Ticket, KRect)> = self
            .held
            .iter()
            .filter(|(k, _, _)| k.theme == theme)
            .filter_map(|(k, &ticket, _)| Some((k.level, ticket, view.place(area, k)?)))
            .collect();
        draw.sort_by_key(|&(l, _, _)| (l == level, l.0));
        let mut request = Vec::new();
        if settled {
            for &key in &missing {
                if self.asked.len() >= self.budget {
                    break;
                }
                if self.asked.values().any(|a| a.key == key) {
                    continue;
                }
                let ticket = NEXT.fetch_add(1, Ordering::Relaxed);
                self.asked.insert(ticket, Asked { key, stale: false });
                request.push((ticket, key));
            }
        }
        Work {
            draw: draw.into_iter().map(|(_, t, r)| (t, r)).collect(),
            request,
            evict: std::mem::take(&mut self.evicted),
            missing: missing.len(),
        }
    }

    /// The painting asked for under `ticket` has arrived: held in place of
    /// the tile's last, which is let go, as are the least recently used past
    /// the cap — or dropped, when the ticket isn't one in flight.
    pub fn arrived(&mut self, ticket: Ticket) -> Arrival {
        let Some(Asked { key, stale }) = self.asked.remove(&ticket) else {
            return Arrival::Dropped;
        };
        self.evicted.extend(self.held.remove(&key));
        let dropped = self.held.insert(key, ticket, 1);
        self.evicted.extend(dropped.into_iter().map(|(_, t)| t));
        if stale {
            self.held.damage_all_where(|k| *k == key);
            Arrival::Stale
        } else {
            Arrival::Current
        }
    }

    /// The tile asked for under `ticket` was painted as nothing — the scene
    /// couldn't be drawn when it was asked of — so it is no longer in
    /// flight, and is asked for again.
    pub fn missed(&mut self, ticket: Ticket) {
        self.asked.remove(&ticket);
    }
}

#[cfg(test)]
mod tests;
