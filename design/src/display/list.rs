//! The list: every chunk by paint order and the top-level R-tree over them
//! in the design's coordinates — what touches a rectangle, in log time —
//! with the extent that keeps the drawing's area without a walk.

use std::collections::{BTreeMap, HashMap};

use kurbo::{BezPath, Rect as KRect, Shape, Vec2};
use rstar::{AABB, RTree, RTreeObject};

use crate::render::{Draw, Frame};

use super::*;

/// What an entry of a space's index is: a chunk, by its order, or a run
/// whose children are spaces of their own.
#[derive(Clone, PartialEq, Debug)]
pub(super) enum What {
    Chunk(Order),
    Run(RunId),
}

/// An entry in a space's index.
#[derive(Clone, PartialEq)]
pub(super) struct Entry {
    pub(super) what: What,
    pub(super) bounds: KRect,
}

impl RTreeObject for Entry {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        aabb(self.bounds)
    }
}

/// A chunk a query found: its order, where its space's origin is in the
/// design's coordinates, and what cuts it from outside its space.
pub(super) struct Found<'a> {
    pub(super) order: &'a Order,
    pub(super) at: Vec2,
    pub(super) cut: Option<KRect>,
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
/// index over the chunks — the design's own space, and a space for each
/// child of each run.
pub struct List {
    pub trees: Trees,
    pub(super) chunks: BTreeMap<Order, Chunk>,
    pub(super) root: SpaceIndex,
    pub(super) runs: Vec<Option<Run>>,
    pub(super) free_runs: Vec<RunId>,
    /// Children whose spaces changed since their runs last summed them.
    pub(super) stale: std::collections::BTreeSet<(RunId, crate::tree::OrderKey)>,
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
            item: None,
        });
        let root_clip = trees.clips.add(Clip {
            parent: None,
            transform: root_transform,
            path: BezPath::new(),
        });
        List {
            trees,
            chunks: BTreeMap::new(),
            root: SpaceIndex::default(),
            runs: Vec::new(),
            free_runs: Vec::new(),
            stale: Default::default(),
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
            Chunk::new(t, Space::Root, Vec2::ZERO, made, Vec::new(), None, None),
        );
        list
    }

    /// Puts the chunks made for a new scene in, those in the design's own
    /// space bulk-loaded into its index with what is in it already.
    pub fn load(&mut self, chunks: Vec<(Order, Chunk)>) {
        let mut entries: Vec<Entry> = std::mem::take(&mut self.root.index).into_iter().collect();
        for (order, chunk) in chunks {
            if chunk.space != Space::Root {
                self.put(order, chunk);
                continue;
            }
            if let Some(r) = chunk.cover() {
                self.root.covers.add(r);
            }
            if let Some(bounds) = chunk.reach() {
                entries.push(Entry {
                    what: What::Chunk(order.clone()),
                    bounds,
                });
            }
            self.chunks.insert(order, chunk);
        }
        self.root.index = RTree::bulk_load(entries);
        self.flush();
    }

    /// The drawing's area becomes `area`, and the root clip with it.
    pub fn set_area(&mut self, area: Frame) {
        self.area = area;
        self.trees.clips.get_mut(self.root_clip).path = frame_rect(area).to_path(0.1);
    }

    /// What the chunks cover together, before rounding.
    pub fn covered(&self) -> Option<KRect> {
        self.root.covers.rect()
    }

    pub fn chunk(&self, order: &Order) -> Option<&Chunk> {
        self.chunks.get(order)
    }

    /// Every chunk, in paint order.
    pub fn chunks(&self) -> impl Iterator<Item = (&Order, &Chunk)> {
        self.chunks.iter()
    }

    /// The index of a space.
    fn local(&mut self, space: &Space) -> &mut SpaceIndex {
        match space {
            Space::Root => &mut self.root,
            Space::Item(r, k) => self.runs[*r as usize]
                .as_mut()
                .expect("a live run")
                .spaces
                .entry(k.clone())
                .or_default(),
        }
    }

    /// Takes the chunk at `order` out of the list and its space's index.
    pub fn take(&mut self, order: &Order) -> Option<Chunk> {
        let chunk = self.chunks.remove(order)?;
        let (reach, cover, space) = (chunk.reach(), chunk.cover(), chunk.space.clone());
        if self.space_lives(&space) {
            let local = self.local(&space);
            if let Some(bounds) = reach {
                local.index.remove(&Entry {
                    what: What::Chunk(order.clone()),
                    bounds,
                });
            }
            if let Some(r) = cover {
                local.covers.remove(r);
            }
            self.touch(&space);
        }
        Some(chunk)
    }

    /// Puts a chunk in the list and its space's index.
    pub fn put(&mut self, order: Order, chunk: Chunk) {
        let (reach, cover, space) = (chunk.reach(), chunk.cover(), chunk.space.clone());
        let local = self.local(&space);
        if let Some(bounds) = reach {
            local.index.insert(Entry {
                what: What::Chunk(order.clone()),
                bounds,
            });
        }
        if let Some(r) = cover {
            local.covers.add(r);
        }
        self.touch(&space);
        self.chunks.insert(order, chunk);
    }

    fn space_lives(&self, space: &Space) -> bool {
        match space {
            Space::Root => true,
            Space::Item(r, _) => self.runs.get(*r as usize).is_some_and(|r| r.is_some()),
        }
    }

    /// Marks a space's run child as needing its run to sum it again.
    fn touch(&mut self, space: &Space) {
        if let Space::Item(r, k) = space {
            self.stale.insert((*r, k.clone()));
        }
    }

    /// A new run, in `space`, with no children yet.
    pub fn add_run(&mut self, space: Space) -> RunId {
        let run = Run::new(space);
        match self.free_runs.pop() {
            Some(id) => {
                self.runs[id as usize] = Some(run);
                id
            }
            None => {
                self.runs.push(Some(run));
                (self.runs.len() - 1) as RunId
            }
        }
    }

    pub fn run(&self, id: RunId) -> &Run {
        self.runs[id as usize].as_ref().expect("a live run")
    }

    pub fn run_mut(&mut self, id: RunId) -> &mut Run {
        self.runs[id as usize].as_mut().expect("a live run")
    }

    /// Takes a run out, its entry and cover with it; its children's chunks
    /// must be gone already.
    pub fn remove_run(&mut self, id: RunId) {
        self.place_run(id, None, None);
        let space = self.run(id).space.clone();
        self.runs[id as usize] = None;
        self.free_runs.push(id);
        self.stale.retain(|(r, _)| *r != id);
        self.touch(&space);
    }

    /// Puts a run's child in its sums, as `placed` — or, with `None`, takes
    /// it out, its space with it.
    pub fn place_child(&mut self, id: RunId, key: crate::tree::OrderKey, placed: Option<Placed>) {
        let run = self.run_mut(id);
        match placed {
            Some(p) => run.set(key.clone(), p),
            None => {
                run.unset(&key);
                run.spaces.remove(&key);
            }
        }
        self.stale.remove(&(id, key));
        self.resum(id);
    }

    /// The run's entry and cover in its space made again from its sums —
    /// after where it places its children changed.
    pub fn resum(&mut self, id: RunId) {
        let (reach, cover) = self.run(id).extents();
        self.place_run(id, reach, cover);
    }

    fn place_run(&mut self, id: RunId, reach: Option<KRect>, cover: Option<KRect>) {
        let (space, old_reach, old_cover) = {
            let run = self.run(id);
            (run.space.clone(), run.entry, run.cover)
        };
        if old_reach == reach && old_cover == cover {
            return;
        }
        let local = self.local(&space);
        if let Some(bounds) = old_reach {
            local.index.remove(&Entry {
                what: What::Run(id),
                bounds,
            });
        }
        if let Some(r) = old_cover {
            local.covers.remove(r);
        }
        if let Some(bounds) = reach {
            local.index.insert(Entry {
                what: What::Run(id),
                bounds,
            });
        }
        if let Some(r) = cover {
            local.covers.add(r);
        }
        let run = self.run_mut(id);
        run.entry = reach;
        run.cover = cover;
        self.touch(&space);
    }

    /// Sums again every run child whose space changed, and every run around
    /// it, out to the design's own space.
    pub fn flush(&mut self) {
        while let Some((r, k)) = self.stale.pop_last() {
            if !self.space_lives(&Space::Item(r, k.clone())) {
                continue;
            }
            let run = self.run_mut(r);
            if let Some(p) = run.items.get(&k).cloned() {
                run.set(k, p);
            }
            self.resum(r);
        }
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
    /// than `pixel` both ways skipped — through every run's children that
    /// can touch it.
    pub(super) fn found(&self, rect: KRect, pixel: f64) -> Vec<Found<'_>> {
        let mut out = Vec::new();
        self.found_in(&self.root, Vec2::ZERO, None, rect, pixel, &mut out);
        out.sort_by(|a, b| a.order.cmp(b.order));
        out
    }

    fn found_in<'a>(
        &'a self,
        local: &'a SpaceIndex,
        at: Vec2,
        cut: Option<KRect>,
        rect: KRect,
        pixel: f64,
        out: &mut Vec<Found<'a>>,
    ) {
        let inside = rect - at;
        for e in local.index.locate_in_envelope_intersecting(&aabb(inside)) {
            if e.bounds.width() < pixel && e.bounds.height() < pixel {
                continue;
            }
            match &e.what {
                What::Chunk(order) => out.push(Found { order, at, cut }),
                What::Run(id) => {
                    let run = self.run(*id);
                    let outer = run.cut.map(|c| c + at);
                    let cut = match (cut, outer) {
                        (Some(a), Some(b)) => Some(a.intersect(b)),
                        (a, b) => a.or(b),
                    };
                    let rect = match outer {
                        Some(c) => rect.intersect(c),
                        None => rect,
                    };
                    if rect.width() < 0.0 || rect.height() < 0.0 {
                        continue;
                    }
                    for (k, child) in run.touching(rect - at) {
                        if let Some(space) = run.spaces.get(k) {
                            self.found_in(space, at + child, cut, rect, pixel, out);
                        }
                    }
                }
            }
        }
    }

    /// The run children that touch `rect` — in the design's coordinates —
    /// and no longer round as their spaces were made: what is rounded anew
    /// before `rect` is painted, hit or read. Only what touches `rect` is
    /// looked at.
    pub fn unrounded(&self, rect: KRect) -> Vec<(RunId, crate::tree::OrderKey)> {
        let mut out = Vec::new();
        self.unrounded_in(&self.root, Vec2::ZERO, rect, &mut out);
        out
    }

    fn unrounded_in(
        &self,
        local: &SpaceIndex,
        at: Vec2,
        rect: KRect,
        out: &mut Vec<(RunId, crate::tree::OrderKey)>,
    ) {
        for e in local
            .index
            .locate_in_envelope_intersecting(&aabb(rect - at))
        {
            let What::Run(id) = &e.what else { continue };
            let run = self.run(*id);
            let rect = match run.cut {
                Some(c) => rect.intersect(c + at),
                None => rect,
            };
            if rect.width() < 0.0 || rect.height() < 0.0 {
                continue;
            }
            for (k, child, fresh) in run.touching_rounded(rect - at) {
                if !fresh {
                    out.push((*id, k.clone()));
                } else if let Some(space) = run.spaces.get(k) {
                    self.unrounded_in(space, at + child, rect, out);
                }
            }
        }
    }

    /// The commands that touch `rect`, in the design's coordinates, in paint
    /// order — a chunk smaller than `pixel` both ways skipped — each with its
    /// chunk and where the chunk's origin is in the design's coordinates.
    pub fn query(&self, rect: KRect, pixel: f64) -> Vec<(Vec2, &Chunk, &Command)> {
        let mut out = Vec::new();
        for f in self.found(rect, pixel) {
            let chunk = &self.chunks[f.order];
            let origin = f.at + chunk.origin;
            let local = rect - origin;
            let mut at: Vec<usize> = chunk
                .index
                .locate_in_envelope_intersecting(&aabb(local))
                .map(|l| l.at)
                .collect();
            at.sort_unstable();
            out.extend(at.into_iter().map(|i| (origin, chunk, &chunk.commands[i])));
        }
        out
    }

    /// Where a space's origin is in the design's coordinates.
    pub fn space_at(&self, space: &Space) -> Vec2 {
        match space {
            Space::Root => Vec2::ZERO,
            Space::Item(r, k) => {
                let run = self.run(*r);
                self.space_at(&run.space) + run.child_at(k).unwrap_or(Vec2::ZERO)
            }
        }
    }

    /// Where `transform`'s origin is in the design's coordinates.
    pub(super) fn world(&self, mut transform: Id) -> Vec2 {
        let mut at = Vec2::ZERO;
        loop {
            let t = self.trees.transforms.get(transform);
            at += t.offset;
            if let Some((r, k)) = &t.item {
                return at + self.space_at(&Space::Item(*r, k.clone()));
            }
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
