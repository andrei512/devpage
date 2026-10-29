//! A run's children in the index: each child a space of its own — its
//! chunks in its own coordinates under an R-tree of its own — placed along
//! the run by the prefix sums of the sizes before it. The run is one entry in
//! the space it sits in; a query inside it finds the first child that
//! reaches the rectangle by a search of the sums, widened by the furthest any
//! child paints past its box, and walks on until a child starts past it. A
//! child that changes size moves no other child's entry: their places are
//! the sums.

use std::collections::{BTreeMap, HashMap};

use kurbo::{Rect as KRect, Vec2};

use crate::prefix_sums::{Item, Summary, Sums};
use crate::tree::OrderKey;

use super::*;

/// A run, by its slot.
pub type RunId = u32;

/// Where a chunk's coordinates are: the design's own, or a run's child's,
/// whose origin is the child's box's top-left.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Space {
    Root,
    Item(RunId, OrderKey),
}

/// A run's children placed along it, and what they paint.
pub struct Run {
    /// The space its own node is in.
    pub space: Space,
    /// Its node's unrounded top-left and its rounded one, in its space.
    pub raw: Vec2,
    pub at: Vec2,
    /// Where its space's origin was in the design's coordinates when it was
    /// last placed: its children are placed in single precision there, as
    /// taffy places them, so they round where a whole tree's do.
    pub origin: Vec2,
    /// Whether its children go along x; else along y.
    pub row: bool,
    /// A frame with no layout: each child at a place of its own, found by
    /// an R-tree of their boxes rather than by the sums.
    pub free: bool,
    pub(super) boxes: rstar::RTree<Boxed>,
    pub(super) free_reach: [Counted; 4],
    pub(super) free_cover: [Counted; 4],
    /// Where its first child starts along the run, from its node's unrounded
    /// top-left: the padding and any `justify` offset.
    pub start: f64,
    /// Where its children's line starts across, from the same: the padding.
    pub across: f64,
    /// How far apart its children are: its gap, and any room
    /// `"space-between"` spreads between them — one number for the run, so
    /// a change of it moves no child's entry.
    pub gap: f64,
    /// The breadth of its children's line.
    pub line: f64,
    /// What cuts it in its space: its clipping frames' boxes.
    pub cut: Option<KRect>,
    pub(super) items: Sums<OrderKey, Placed>,
    pub(super) spaces: HashMap<OrderKey, SpaceIndex>,
    /// The furthest any child reaches past its box along the run, and each
    /// child's reach and cover across, from the line's start: counted sets.
    pub(super) overhang: Counted,
    pub(super) reach_across: (Counted, Counted),
    pub(super) cover_across: (Counted, Counted),
    /// Its entry in its space's index, and what it adds to its space's cover.
    pub(super) entry: Option<KRect>,
    pub(super) cover: Option<KRect>,
}

/// One space's index: its chunks and runs by their bounds in its own
/// coordinates, and the extent of what its chunks cover.
#[derive(Default)]
pub struct SpaceIndex {
    pub(super) index: rstar::RTree<Entry>,
    pub(super) covers: Extent,
}

/// A child of a run as the run's sums see it: its size along the run, where
/// it sits across, and what it reaches and covers along the run from its
/// unrounded start.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub size: f64,
    /// In a free run, its unrounded top-left from the run's node's.
    pub at: Option<Vec2>,
    /// In a free run, what it reaches and covers in the run's space.
    pub(super) boxed: Option<KRect>,
    pub(super) cover_box: Option<KRect>,
    /// Its unrounded place across, from the line's start.
    pub across: f64,
    /// Its rounded top-left less its unrounded one: fixed while what the
    /// run places it at keeps its fraction of a pixel.
    pub delta: Vec2,
    pub(super) reach: Option<(f64, f64)>,
    pub(super) covered: Option<(f64, f64)>,
    /// What it put in the run's counted sets, to take out again.
    pub(super) counted: Counts,
}

/// A child's entries in its run's counted sets.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Counts {
    overhang: Option<f64>,
    reach: Option<(f64, f64)>,
    cover: Option<(f64, f64)>,
}

impl Placed {
    /// A free run's child, at `at` from the run's node's top-left.
    pub fn at(at: Vec2, delta: Vec2) -> Placed {
        Placed {
            at: Some(at),
            ..Placed::new(0.0, 0.0, delta)
        }
    }

    pub fn new(size: f64, across: f64, delta: Vec2) -> Placed {
        Placed {
            at: None,
            boxed: None,
            cover_box: None,
            size,
            across,
            delta,
            reach: None,
            covered: None,
            counted: Counts::default(),
        }
    }
}

/// A stretch of a run's children: how many, and the sum of their sizes —
/// how far they advance less the gaps, which are the run's.
#[derive(Clone, Debug, Default)]
pub struct Span {
    pub n: usize,
    pub length: f64,
}

impl Summary for Span {
    fn then(&self, next: &Self) -> Self {
        Span {
            n: self.n + next.n,
            length: self.length + next.length,
        }
    }
}

impl Item for Placed {
    type Sum = Span;
    fn summary(&self) -> Span {
        Span {
            n: 1,
            length: self.size,
        }
    }
}

/// A free run's child's entry in the run's R-tree: what it reaches.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Boxed {
    pub(super) key: OrderKey,
    pub(super) bounds: KRect,
}

impl rstar::RTreeObject for Boxed {
    type Envelope = rstar::AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        rstar::AABB::from_corners(
            [self.bounds.x0, self.bounds.y0],
            [self.bounds.x1, self.bounds.y1],
        )
    }
}

/// A multiset of numbers that knows its least and greatest in log time.
#[derive(Default)]
pub struct Counted(BTreeMap<i64, usize>);

impl Counted {
    pub(super) fn add(&mut self, v: f64) {
        *self.0.entry(key(v)).or_default() += 1;
    }
    pub(super) fn remove(&mut self, v: f64) {
        let k = key(v);
        if let Some(n) = self.0.get_mut(&k) {
            *n -= 1;
            if *n == 0 {
                self.0.remove(&k);
            }
        }
    }
    pub(super) fn least(&self) -> Option<f64> {
        self.0.keys().next().map(|k| *k as f64 / 1e6)
    }
    pub(super) fn greatest(&self) -> Option<f64> {
        self.0.keys().next_back().map(|k| *k as f64 / 1e6)
    }
}

/// A rectangle's span along a run and across it.
fn spans(row: bool, r: KRect) -> ((f64, f64), (f64, f64)) {
    if row {
        ((r.x0, r.x1), (r.y0, r.y1))
    } else {
        ((r.y0, r.y1), (r.x0, r.x1))
    }
}

fn rect(row: bool, along: (f64, f64), across: (f64, f64)) -> KRect {
    if row {
        KRect::new(along.0, across.0, along.1, across.1)
    } else {
        KRect::new(across.0, along.0, across.1, along.1)
    }
}

impl Run {
    pub fn new(space: Space) -> Run {
        Run {
            space,
            raw: Vec2::ZERO,
            at: Vec2::ZERO,
            origin: Vec2::ZERO,
            row: false,
            free: false,
            boxes: rstar::RTree::new(),
            free_reach: Default::default(),
            free_cover: Default::default(),
            start: 0.0,
            across: 0.0,
            gap: 0.0,
            line: 0.0,
            cut: None,
            items: Sums::new(),
            spaces: HashMap::new(),
            overhang: Counted::default(),
            reach_across: Default::default(),
            cover_across: Default::default(),
            entry: None,
            cover: None,
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Where the run places a child: its unrounded top-left in the run's
    /// space, and its rounded one — each edge where taffy's pass puts it,
    /// measured from the run's node, a box in the flow.
    /// Where a free run places a child at `at` from its node: its unrounded
    /// top-left and its rounded one, measured from the run's node, as taffy
    /// rounds a box placed in its parent.
    pub fn place_at(&self, at: Vec2) -> (Vec2, Vec2) {
        let o = self.origin;
        let (rx, ry) = ((o.x + self.raw.x) as f32, (o.y + self.raw.y) as f32);
        let raw = (rx + at.x as f32, ry + at.y as f32);
        let snap32 = |v: f32| (v + 0.5).floor();
        let rounded = (
            (o.x + self.at.x) as f32 + snap32(raw.0) - snap32(rx),
            (o.y + self.at.y) as f32 + snap32(raw.1) - snap32(ry),
        );
        (
            Vec2::new(raw.0 as f64 - o.x, raw.1 as f64 - o.y),
            Vec2::new(rounded.0 as f64 - o.x, rounded.1 as f64 - o.y),
        )
    }

    pub fn place(&self, before: f64, across: f64) -> (Vec2, Vec2) {
        // In single precision, as taffy places a box and its rounding pass
        // rounds it: the child's place within the run, then the run's.
        let along = (self.start + before) as f32;
        let across = (self.across + across) as f32;
        let (dx, dy) = if self.row {
            (along, across)
        } else {
            (across, along)
        };
        let o = self.origin;
        let (rx, ry) = ((o.x + self.raw.x) as f32, (o.y + self.raw.y) as f32);
        let raw = (rx + dx, ry + dy);
        let snap32 = |v: f32| (v + 0.5).floor();
        let at = (
            (o.x + self.at.x) as f32 + snap32(raw.0) - snap32(rx),
            (o.y + self.at.y) as f32 + snap32(raw.1) - snap32(ry),
        );
        (
            Vec2::new(raw.0 as f64 - o.x, raw.1 as f64 - o.y),
            Vec2::new(at.0 as f64 - o.x, at.1 as f64 - o.y),
        )
    }

    /// The child at `key`: how far along the run the children before it
    /// advance, and how it is placed.
    pub fn child(&self, key: &OrderKey) -> Option<(f64, &Placed)> {
        let placed = self.items.get(key)?;
        Some((self.prefix(key), placed))
    }

    /// How far along the run the children before `key` advance, gaps and
    /// all.
    pub fn prefix(&self, key: &OrderKey) -> f64 {
        let s = self.items.before(key);
        s.length + s.n as f64 * self.gap
    }

    /// The keys of the children after `key`, in order.
    pub fn keys_after(&self, key: &OrderKey) -> Vec<OrderKey> {
        self.items
            .from(key)
            .filter(|(k, _)| *k != key)
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// Its entry's bounds in its space's index.
    pub fn entry(&self) -> Option<KRect> {
        self.entry
    }

    /// Where the child at `key` is: its rounded top-left in the run's space.
    pub fn child_at(&self, key: &OrderKey) -> Option<Vec2> {
        if self.free {
            let p = self.items.get(key)?;
            return Some(self.place_at(p.at.unwrap_or(Vec2::ZERO)).1);
        }
        let (before, p) = self.child(key)?;
        Some(self.place(before, p.across).1)
    }

    /// The children in order, each with how far the ones before it advance.
    pub fn children(&self) -> impl Iterator<Item = (&OrderKey, f64, &Placed)> {
        let mut before = 0.0;
        let gap = self.gap;
        self.items.iter().map(move |(k, p)| {
            let b = before;
            before += p.size + gap;
            (k, b, p)
        })
    }

    /// The children that can touch `r`, in the run's space, in order: from
    /// the first whose box ends past `r`'s start, less the furthest any child
    /// reaches past its box, to the last that starts before its end, plus
    /// that.
    pub(super) fn touching(&self, r: KRect) -> Vec<(&OrderKey, Vec2)> {
        self.touching_rounded(r)
            .into_iter()
            .map(|(k, at, _)| (k, at))
            .collect()
    }

    /// [`Run::touching`], each child with whether it still rounds as its
    /// space was made: whether its rounded place less its unrounded one is
    /// what it was when its boxes and chunks were made. A child the sums
    /// moved by a fraction of a pixel doesn't, and is rounded anew before it
    /// is read.
    pub(super) fn touching_rounded(&self, r: KRect) -> Vec<(&OrderKey, Vec2, bool)> {
        if self.free {
            let env = rstar::AABB::from_corners([r.x0, r.y0], [r.x1, r.y1]);
            let mut found: Vec<&Boxed> = self.boxes.locate_in_envelope_intersecting(&env).collect();
            found.sort_by(|a, b| a.key.cmp(&b.key));
            return found
                .into_iter()
                .filter_map(|b| {
                    let (k, p) = (&b.key, self.items.get(&b.key)?);
                    let (raw, rounded) = self.place_at(p.at.unwrap_or(Vec2::ZERO));
                    let fresh = ((rounded - raw) - p.delta).hypot() < 1e-3;
                    Some((k, rounded, fresh))
                })
                .collect();
        }
        let ((a0, a1), _) = spans(self.row, r);
        let base = if self.row { self.raw.x } else { self.raw.y } + self.start;
        let over = self.overhang.greatest().unwrap_or(0.0).max(0.0);
        let (from, to) = (a0 - base - over, a1 - base + over);
        let gap = self.gap;
        let Some((first, _, before)) = self
            .items
            .seek(|s| s.length + s.n.saturating_sub(1) as f64 * gap >= from)
        else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut at = before.length + before.n as f64 * gap;
        for (k, p) in self.items.from(first) {
            if at > to {
                break;
            }
            let (raw, rounded) = self.place(at, p.across);
            let fresh = ((rounded - raw) - p.delta).hypot() < 1e-3;
            out.push((k, rounded, fresh));
            at += p.size + gap;
        }
        out
    }

    /// What the run's children reach and cover in its space, from its sums
    /// and counted sets; `None` where nothing. Along the run, the least is
    /// found walking in from the first child and the greatest from the
    /// last, each only while a child could still reach past the best found
    /// — no further than the furthest any child reaches past its box.
    pub(super) fn extents(&self) -> (Option<KRect>, Option<KRect>) {
        if self.free {
            let make = |c: &[Counted; 4]| {
                let (Some(x0), Some(y0), Some(x1), Some(y1)) =
                    (c[0].least(), c[1].least(), c[2].greatest(), c[3].greatest())
                else {
                    return None;
                };
                Some(KRect::new(x0, y0, x1, y1) + self.raw)
            };
            let cover = make(&self.free_cover).and_then(|r| super::chunk::cut_to(r, self.cut));
            return (make(&self.free_reach), cover);
        }
        let base = if self.row { self.raw.x } else { self.raw.y } + self.start;
        let across = if self.row { self.raw.y } else { self.raw.x } + self.across;
        let over = self.overhang.greatest().unwrap_or(0.0).max(0.0);
        let along = |of: fn(&Placed) -> Option<(f64, f64)>| -> Option<(f64, f64)> {
            let gap = self.gap;
            let mut lo = f64::INFINITY;
            let mut at = 0.0;
            for (_, p) in self.items.iter() {
                if at - over > lo {
                    break;
                }
                if let Some((l, _)) = of(p) {
                    lo = lo.min(at + l);
                }
                at += p.size + gap;
            }
            let total = self.items.total();
            let mut hi = f64::NEG_INFINITY;
            let mut end = total.length + total.n.saturating_sub(1) as f64 * gap;
            for (_, p) in self.items.rev() {
                let start = end - p.size;
                if end + over < hi {
                    break;
                }
                if let Some((_, h)) = of(p) {
                    hi = hi.max(start + h);
                }
                end = start - gap;
            }
            (lo <= hi).then_some((lo, hi))
        };
        let make = |along: Option<(f64, f64)>, lo: &Counted, hi: &Counted| {
            let along = along?;
            let (Some(c0), Some(c1)) = (lo.least(), hi.greatest()) else {
                return None;
            };
            Some(rect(
                self.row,
                (base + along.0, base + along.1),
                (across + c0, across + c1),
            ))
        };
        let reach = make(
            along(|p| p.reach),
            &self.reach_across.0,
            &self.reach_across.1,
        );
        let cover = make(
            along(|p| p.covered),
            &self.cover_across.0,
            &self.cover_across.1,
        )
        .and_then(|r| super::chunk::cut_to(r, self.cut));
        (reach, cover)
    }

    /// Puts what a child is, from its space's index and covers, in the sums
    /// and the counted sets.
    pub(super) fn set(&mut self, key: OrderKey, mut placed: Placed) {
        self.unset(&key);
        if self.free {
            let local = self.spaces.entry(key.clone()).or_default();
            let env = local.index.root().envelope();
            let reach = (env.lower()[0] <= env.upper()[0]).then(|| {
                KRect::new(
                    env.lower()[0],
                    env.lower()[1],
                    env.upper()[0],
                    env.upper()[1],
                )
            });
            let covered = local.covers.rect();
            // In the run's own coordinates, from its node's unrounded
            // top-left: the child's rounded place less that.
            let at = placed.at.unwrap_or(Vec2::ZERO) + placed.delta;
            placed.boxed = reach.map(|r| r + at);
            placed.cover_box = covered.map(|r| r + at);
            if let Some(b) = placed.boxed {
                self.boxes.insert(Boxed {
                    key: key.clone(),
                    bounds: b,
                });
                for (c, v) in self.free_reach.iter_mut().zip([b.x0, b.y0, b.x1, b.y1]) {
                    c.add(v);
                }
            }
            if let Some(b) = placed.cover_box {
                for (c, v) in self.free_cover.iter_mut().zip([b.x0, b.y0, b.x1, b.y1]) {
                    c.add(v);
                }
            }
            self.items.insert(key, placed);
            return;
        }
        let local = self.spaces.entry(key.clone()).or_default();
        let reach = local.index.root().envelope();
        let reach = (reach.lower()[0] <= reach.upper()[0]).then(|| {
            KRect::new(
                reach.lower()[0],
                reach.lower()[1],
                reach.upper()[0],
                reach.upper()[1],
            )
        });
        let covered = local.covers.rect();
        let d = if self.row {
            (placed.delta.x, placed.delta.y)
        } else {
            (placed.delta.y, placed.delta.x)
        };
        let along = |r: KRect| {
            let ((a0, a1), _) = spans(self.row, r);
            (a0 + d.0, a1 + d.0)
        };
        let across = |r: KRect| {
            let (_, (c0, c1)) = spans(self.row, r);
            (placed.across + d.1 + c0, placed.across + d.1 + c1)
        };
        placed.reach = reach.map(along);
        placed.covered = covered.map(along);
        placed.counted = Counts::default();
        if let Some(r) = reach {
            let (lo, hi) = along(r);
            let o = (-lo).max(hi - placed.size).max(0.0);
            self.overhang.add(o);
            let c = across(r);
            self.reach_across.0.add(c.0);
            self.reach_across.1.add(c.1);
            placed.counted.overhang = Some(o);
            placed.counted.reach = Some(c);
        }
        if let Some(r) = covered {
            let c = across(r);
            self.cover_across.0.add(c.0);
            self.cover_across.1.add(c.1);
            placed.counted.cover = Some(c);
        }
        self.items.insert(key, placed);
    }

    /// Takes a child out of the sums and the counted sets, keeping its
    /// space; returns how it was placed.
    pub(super) fn unset(&mut self, key: &OrderKey) -> Option<Placed> {
        let p = self.items.remove(key)?;
        if let Some(b) = p.boxed {
            self.boxes.remove(&Boxed {
                key: key.clone(),
                bounds: b,
            });
            for (c, v) in self.free_reach.iter_mut().zip([b.x0, b.y0, b.x1, b.y1]) {
                c.remove(v);
            }
        }
        if let Some(b) = p.cover_box {
            for (c, v) in self.free_cover.iter_mut().zip([b.x0, b.y0, b.x1, b.y1]) {
                c.remove(v);
            }
        }
        if let Some(o) = p.counted.overhang {
            self.overhang.remove(o);
        }
        if let Some((c0, c1)) = p.counted.reach {
            self.reach_across.0.remove(c0);
            self.reach_across.1.remove(c1);
        }
        if let Some((c0, c1)) = p.counted.cover {
            self.cover_across.0.remove(c0);
            self.cover_across.1.remove(c1);
        }
        Some(p)
    }
}
