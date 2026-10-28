//! The list: every chunk by paint order and the top-level R-tree over them
//! in the design's coordinates — what touches a rectangle, in log time —
//! with the extent that keeps the drawing's area without a walk.

use std::collections::{BTreeMap, HashMap};

use kurbo::{BezPath, Rect as KRect, Shape, Vec2};
use rstar::{AABB, RTree, RTreeObject};

use crate::render::{Draw, Frame};

use super::*;

/// A chunk's entry in the top-level index.
#[derive(Clone, PartialEq)]
pub(super) struct Entry {
    pub(super) order: Order,
    pub(super) bounds: KRect,
}

impl RTreeObject for Entry {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        aabb(self.bounds)
    }
}

/// A multiset of edges that knows its least and greatest in log time: the
/// drawing's area kept from its chunks' covers as they come and go.
#[derive(Default)]
pub(super) struct Extent {
    x0: BTreeMap<i64, usize>,
    y0: BTreeMap<i64, usize>,
    x1: BTreeMap<i64, usize>,
    y1: BTreeMap<i64, usize>,
}

/// An edge as a key: to a millionth of a pixel, exactly ordered.
pub(super) fn key(v: f64) -> i64 {
    (v * 1e6).round() as i64
}

impl Extent {
    pub(super) fn add(&mut self, r: KRect) {
        for (m, v) in [
            (&mut self.x0, r.x0),
            (&mut self.y0, r.y0),
            (&mut self.x1, r.x1),
            (&mut self.y1, r.y1),
        ] {
            *m.entry(key(v)).or_default() += 1;
        }
    }

    pub(super) fn remove(&mut self, r: KRect) {
        for (m, v) in [
            (&mut self.x0, r.x0),
            (&mut self.y0, r.y0),
            (&mut self.x1, r.x1),
            (&mut self.y1, r.y1),
        ] {
            let k = key(v);
            if let Some(n) = m.get_mut(&k) {
                *n -= 1;
                if *n == 0 {
                    m.remove(&k);
                }
            }
        }
    }

    pub(super) fn rect(&self) -> Option<KRect> {
        let low = |m: &BTreeMap<i64, usize>| m.keys().next().map(|k| *k as f64 / 1e6);
        let high = |m: &BTreeMap<i64, usize>| m.keys().next_back().map(|k| *k as f64 / 1e6);
        Some(KRect::new(
            low(&self.x0)?,
            low(&self.y0)?,
            high(&self.x1)?,
            high(&self.y1)?,
        ))
    }
}

/// The display list: chunks by paint order, the property trees, and the
/// index over the chunks.
pub struct List {
    pub trees: Trees,
    pub(super) chunks: BTreeMap<Order, Chunk>,
    pub(super) index: RTree<Entry>,
    pub(super) covers: Extent,
    /// The root of the transform tree: the design's own coordinates.
    pub root_transform: Id,
    /// The root of the clip tree: the drawing's own area.
    pub root_clip: Id,
    /// The drawing's area.
    pub area: Frame,
}

impl Default for List {
    fn default() -> Self {
        List::new()
    }
}

impl List {
    /// An empty list: the root transform, and the root clip — to an empty
    /// area until [`List::set_area`].
    pub fn new() -> List {
        let mut trees = Trees::default();
        let root_transform = trees.transforms.add(Transform {
            parent: None,
            offset: Vec2::ZERO,
        });
        let root_clip = trees.clips.add(Clip {
            parent: None,
            transform: root_transform,
            path: BezPath::new(),
        });
        List {
            trees,
            chunks: BTreeMap::new(),
            index: RTree::new(),
            covers: Extent::default(),
            root_transform,
            root_clip,
            area: Frame {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
        }
    }

    /// A list of one chunk at the origin, `area` its drawing, built by
    /// `build`: for painting a few commands by themselves.
    pub fn of(area: Frame, build: impl FnOnce(&mut Builder)) -> List {
        let mut list = List::new();
        list.set_area(area);
        let (t, c) = (list.root_transform, list.root_clip);
        let mut b = Builder::new(&mut list.trees, t, c, None);
        build(&mut b);
        let made = b.finish();
        list.put(
            Vec::new(),
            Chunk::new(t, Vec2::ZERO, made, Vec::new(), None, None),
        );
        list
    }

    /// Puts the chunks made for a new scene in, the index bulk-loaded.
    pub fn load(&mut self, chunks: Vec<(Order, Chunk)>) {
        let mut entries = Vec::with_capacity(chunks.len());
        for (order, chunk) in chunks {
            if let Some(r) = chunk.cover() {
                self.covers.add(r);
            }
            if let Some(bounds) = chunk.reach() {
                entries.push(Entry {
                    order: order.clone(),
                    bounds,
                });
            }
            self.chunks.insert(order, chunk);
        }
        self.index = RTree::bulk_load(entries);
    }

    /// The drawing's area becomes `area`, and the root clip with it.
    pub fn set_area(&mut self, area: Frame) {
        self.area = area;
        self.trees.clips.get_mut(self.root_clip).path = frame_rect(area).to_path(0.1);
    }

    /// What the chunks cover together, before rounding.
    pub fn covered(&self) -> Option<KRect> {
        self.covers.rect()
    }

    pub fn chunk(&self, order: &Order) -> Option<&Chunk> {
        self.chunks.get(order)
    }

    /// Every chunk, in paint order.
    pub fn chunks(&self) -> impl Iterator<Item = (&Order, &Chunk)> {
        self.chunks.iter()
    }

    /// Takes the chunk at `order` out of the list and the index.
    pub fn take(&mut self, order: &Order) -> Option<Chunk> {
        let chunk = self.chunks.remove(order)?;
        if let Some(bounds) = chunk.reach() {
            self.index.remove(&Entry {
                order: order.clone(),
                bounds,
            });
        }
        if let Some(r) = chunk.cover() {
            self.covers.remove(r);
        }
        Some(chunk)
    }

    /// Puts a chunk in the list and the index.
    pub fn put(&mut self, order: Order, chunk: Chunk) {
        if let Some(bounds) = chunk.reach() {
            self.index.insert(Entry {
                order: order.clone(),
                bounds,
            });
        }
        if let Some(r) = chunk.cover() {
            self.covers.add(r);
        }
        self.chunks.insert(order, chunk);
    }

    /// Frees the property-tree nodes a chunk's own layers made.
    pub fn free(&mut self, made: &Made) {
        for &c in &made.clips {
            self.trees.clips.remove(c);
        }
        for &e in &made.effects {
            self.trees.effects.remove(e);
        }
    }

    /// The chunks whose entries touch `rect`, in paint order — those smaller
    /// than `pixel` both ways skipped.
    pub(super) fn found(&self, rect: KRect, pixel: f64) -> Vec<&Order> {
        let mut found: Vec<&Order> = self
            .index
            .locate_in_envelope_intersecting(&aabb(rect))
            .filter(|e| e.bounds.width() >= pixel || e.bounds.height() >= pixel)
            .map(|e| &e.order)
            .collect();
        found.sort();
        found
    }

    /// The commands that touch `rect`, in the design's coordinates, in paint
    /// order — a chunk smaller than `pixel` both ways skipped — each with its
    /// chunk.
    pub fn query(&self, rect: KRect, pixel: f64) -> Vec<(&Chunk, &Command)> {
        let mut out = Vec::new();
        for order in self.found(rect, pixel) {
            let chunk = &self.chunks[order];
            let local = rect - chunk.origin;
            let mut at: Vec<usize> = chunk
                .index
                .locate_in_envelope_intersecting(&aabb(local))
                .map(|l| l.at)
                .collect();
            at.sort_unstable();
            out.extend(at.into_iter().map(|i| (chunk, &chunk.commands[i])));
        }
        out
    }

    /// Where `transform`'s origin is in the design's coordinates.
    pub(super) fn world(&self, mut transform: Id) -> Vec2 {
        let mut at = Vec2::ZERO;
        loop {
            let t = self.trees.transforms.get(transform);
            at += t.offset;
            match t.parent {
                Some(p) => transform = p,
                None => return at,
            }
        }
    }

    /// Every image the list paints swapped for the blob `last_of` names for
    /// it, where it names one.
    pub fn swap_images(&mut self, last_of: &HashMap<u64, peniko::ImageData>) {
        for chunk in self.chunks.values_mut() {
            for c in &mut chunk.commands {
                if let Draw::Fill(_, crate::render::Paint::Image(data, _)) = &mut c.draw
                    && let Some(old) = last_of.get(&data.data.id())
                {
                    *data = old.clone();
                }
            }
        }
    }
}

pub(super) fn frame_rect(f: Frame) -> KRect {
    KRect::new(
        f.x as f64,
        f.y as f64,
        (f.x + f.width) as f64,
        (f.y + f.height) as f64,
    )
}

#[cfg(test)]
mod tests;
