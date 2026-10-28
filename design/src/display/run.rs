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

use crate::sums::{Item, Summary, Sums};
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
    /// Whether its children go along x; else along y.
    pub row: bool,
    /// Where its first child starts along the run, from its node's unrounded
    /// top-left: the padding and any `justify` offset.
    pub start: f64,
    /// Where its children's line starts across, from the same: the padding.
    pub across: f64,
    /// The gap between children.
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

/// A child of a run as the run's sums see it: its size along the run, the
/// gap after it, where it sits across, and what it reaches and covers along
/// the run from its unrounded start.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub size: f64,
    pub gap: f64,
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
    pub fn new(size: f64, gap: f64, across: f64, delta: Vec2) -> Placed {
        Placed {
            size,
            gap,
            across,
            delta,
            reach: None,
            covered: None,
            counted: Counts::default(),
        }
    }
}

/// A stretch of a run's children: how many, how far they advance — each
/// child's size and the gap after it — and the least and greatest each
/// reaches and covers, along the run from the stretch's start.
#[derive(Clone, Debug)]
pub struct Span {
    pub n: usize,
    pub length: f64,
    pub reach: (f64, f64),
    pub covered: (f64, f64),
}

impl Default for Span {
    fn default() -> Self {
        Span {
            n: 0,
            length: 0.0,
            reach: (f64::INFINITY, f64::NEG_INFINITY),
            covered: (f64::INFINITY, f64::NEG_INFINITY),
        }
    }
}

impl Summary for Span {
    fn then(&self, next: &Self) -> Self {
        let shift = |(lo, hi): (f64, f64), (nlo, nhi): (f64, f64)| {
            (lo.min(self.length + nlo), hi.max(self.length + nhi))
        };
        Span {
            n: self.n + next.n,
            length: self.length + next.length,
            reach: shift(self.reach, next.reach),
            covered: shift(self.covered, next.covered),
        }
    }
}

impl Item for Placed {
    type Sum = Span;
    fn summary(&self) -> Span {
        let none = (f64::INFINITY, f64::NEG_INFINITY);
        Span {
            n: 1,
            length: self.size + self.gap,
            reach: self.reach.unwrap_or(none),
            covered: self.covered.unwrap_or(none),
        }
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

/// `v` along the run, `w` across, as a point in `row`'s axes.
fn point(row: bool, along: f64, across: f64) -> Vec2 {
    if row {
        Vec2::new(along, across)
    } else {
        Vec2::new(across, along)
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

/// Rounds as taffy rounds: to the nearest whole pixel, a half up.
pub(crate) fn snap(v: f64) -> f64 {
    (v + 0.5).floor()
}

impl Run {
    pub fn new(space: Space) -> Run {
        Run {
            space,
            raw: Vec2::ZERO,
            at: Vec2::ZERO,
            row: false,
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

    /// A child's unrounded top-left in the run's space, its prefix along the
    /// run given.
    fn raw_of(&self, before: f64, across: f64) -> Vec2 {
        self.raw + point(self.row, self.start + before, self.across + across)
    }

    /// Where the run places a child: its unrounded top-left in the run's
    /// space, and its rounded one — each edge where taffy's pass puts it,
    /// measured from the run's node, a box in the flow.
    pub fn place(&self, before: f64, across: f64) -> (Vec2, Vec2) {
        let raw = self.raw_of(before, across);
        let at = Vec2::new(
            self.at.x + snap(raw.x) - snap(self.raw.x),
            self.at.y + snap(raw.y) - snap(self.raw.y),
        );
        (raw, at)
    }

    /// The child at `key`: how far along the run the children before it
    /// advance, and how it is placed.
    pub fn child(&self, key: &OrderKey) -> Option<(f64, &Placed)> {
        let placed = self.items.get(key)?;
        Some((self.items.before(key).length, placed))
    }

    /// How far along the run the children before `key` advance.
    pub fn prefix(&self, key: &OrderKey) -> f64 {
        self.items.before(key).length
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
        let (before, p) = self.child(key)?;
        Some(self.place(before, p.across).1)
    }

    /// The children in order, each with how far the ones before it advance.
    pub fn children(&self) -> impl Iterator<Item = (&OrderKey, f64, &Placed)> {
        let mut before = 0.0;
        self.items.iter().map(move |(k, p)| {
            let b = before;
            before += p.size + p.gap;
            (k, b, p)
        })
    }

    /// The children that can touch `r`, in the run's space, in order: from
    /// the first whose box ends past `r`'s start, less the furthest any child
    /// reaches past its box, to the last that starts before its end, plus
    /// that.
    pub(super) fn touching(&self, r: KRect) -> Vec<(&OrderKey, Vec2)> {
        let ((a0, a1), _) = spans(self.row, r);
        let base = if self.row { self.raw.x } else { self.raw.y } + self.start;
        let over = self.overhang.greatest().unwrap_or(0.0).max(0.0);
        let (from, to) = (a0 - base - over, a1 - base + over);
        let Some((first, _, before)) = self.items.seek(|s| s.length - s_gap(s, self.gap) >= from)
        else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut at = before.length;
        for (k, p) in self.items.from(first) {
            if at > to {
                break;
            }
            out.push((k, self.place(at, p.across).1));
            at += p.size + p.gap;
        }
        out
    }

    /// What the run's children reach and cover in its space, from its sums
    /// and counted sets; `None` where nothing.
    pub(super) fn extents(&self) -> (Option<KRect>, Option<KRect>) {
        let total = self.items.total();
        let base = if self.row { self.raw.x } else { self.raw.y } + self.start;
        let across = if self.row { self.raw.y } else { self.raw.x } + self.across;
        let make = |along: (f64, f64), lo: &Counted, hi: &Counted| {
            let (Some(c0), Some(c1)) = (lo.least(), hi.greatest()) else {
                return None;
            };
            if !(along.0 <= along.1) {
                return None;
            }
            Some(rect(
                self.row,
                (base + along.0, base + along.1),
                (across + c0, across + c1),
            ))
        };
        let reach = make(total.reach, &self.reach_across.0, &self.reach_across.1);
        let cover = make(total.covered, &self.cover_across.0, &self.cover_across.1)
            .and_then(|r| super::chunk::cut_to(r, self.cut));
        (reach, cover)
    }

    /// Puts what a child is, from its space's index and covers, in the sums
    /// and the counted sets.
    pub(super) fn set(&mut self, key: OrderKey, mut placed: Placed) {
        self.unset(&key);
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

/// The gap a stretch's length counts after its last child.
fn s_gap(s: &Span, gap: f64) -> f64 {
    if s.n == 0 { 0.0 } else { gap }
}
