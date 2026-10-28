//! The display list: drawing commands in chunks — one per scene node, in the
//! node's own coordinates — under property trees, and the two-level index
//! over them. A command doesn't nest layers; it names its clip and its
//! effect, and [`paint`] opens and closes exactly the layers each command it
//! paints needs, so any subset of the list paints in order. Every tile, PNG
//! band and click goes through here.

use std::collections::{BTreeMap, HashMap};

use rstar::{AABB, RTree, RTreeObject};
use vello_cpu::kurbo::{Affine, BezPath, Rect as KRect, Shape, Vec2};

use crate::render::{Draw, Frame};

/// A node in one of the property trees, by its slot.
pub type Id = u32;

/// A scene node's place: its offset from its parent's.
#[derive(Clone, Debug)]
pub struct Transform {
    pub parent: Option<Id>,
    pub offset: Vec2,
}

/// A clipping shape, in the coordinates of its transform node.
#[derive(Clone)]
pub struct Clip {
    pub parent: Option<Id>,
    pub transform: Id,
    pub path: BezPath,
}

/// How an effect's layer meets what is under it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Blend {
    /// Drawn over it at an opacity.
    Opacity(f32),
    /// Added to it, color and alpha alike, so two fills in such layers that
    /// meet edge to edge sum to exactly what is in them, with no seam.
    Add,
}

/// A layer: its blend, the bound nothing in it reaches — in its transform's
/// coordinates, or, when `None`, the whole area being painted — and the clip
/// it is drawn inside.
#[derive(Clone)]
pub struct Effect {
    pub parent: Option<Id>,
    pub clip: Id,
    pub transform: Id,
    pub blend: Blend,
    pub bounds: Option<BezPath>,
}

/// A store of property-tree nodes whose slots are reused.
#[derive(Clone)]
pub struct Arena<T> {
    slots: Vec<Option<T>>,
    free: Vec<Id>,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Arena {
            slots: Vec::new(),
            free: Vec::new(),
        }
    }
}

impl<T> Arena<T> {
    pub fn add(&mut self, t: T) -> Id {
        match self.free.pop() {
            Some(id) => {
                self.slots[id as usize] = Some(t);
                id
            }
            None => {
                self.slots.push(Some(t));
                (self.slots.len() - 1) as Id
            }
        }
    }

    pub fn remove(&mut self, id: Id) {
        if self
            .slots
            .get_mut(id as usize)
            .and_then(Option::take)
            .is_some()
        {
            self.free.push(id);
        }
    }

    pub fn get(&self, id: Id) -> &T {
        self.slots[id as usize].as_ref().expect("a live node")
    }

    pub fn get_mut(&mut self, id: Id) -> &mut T {
        self.slots[id as usize].as_mut().expect("a live node")
    }
}

/// The three property trees.
#[derive(Clone, Default)]
pub struct Trees {
    pub transforms: Arena<Transform>,
    pub clips: Arena<Clip>,
    pub effects: Arena<Effect>,
}

/// A drawing command and the property state it is drawn under: its clip and
/// its effect, if any. Its transform is its chunk's.
pub struct Command {
    pub draw: Draw,
    pub clip: Id,
    pub effect: Option<Id>,
    /// Every pixel it can touch, in its chunk's coordinates.
    pub bounds: KRect,
}

/// Builds one chunk's commands, turning the layers they are drawn in into
/// property-tree nodes under the chunk's transform.
pub struct Builder<'a> {
    trees: &'a mut Trees,
    transform: Id,
    stack: Vec<(Id, Option<Id>)>,
    commands: Vec<Command>,
    /// The property-tree nodes made for layers, to be freed with the chunk.
    made: Made,
}

/// The property-tree nodes a chunk's own layers made.
#[derive(Default, Clone)]
pub struct Made {
    pub clips: Vec<Id>,
    pub effects: Vec<Id>,
}

impl<'a> Builder<'a> {
    /// Commands drawn under `transform`, inside `clip` and `effect`.
    pub fn new(trees: &'a mut Trees, transform: Id, clip: Id, effect: Option<Id>) -> Builder<'a> {
        Builder {
            trees,
            transform,
            stack: vec![(clip, effect)],
            commands: Vec::new(),
            made: Made::default(),
        }
    }

    /// A fill or a stroke, covering what its path covers.
    pub fn draw(&mut self, draw: Draw) {
        let bounds = match &draw {
            Draw::Fill(path, _) => path.bounding_box(),
            Draw::Stroke(path, width, _) => {
                let half = *width as f64 / 2.0;
                path.bounding_box().inflate(half, half)
            }
            Draw::Glyphs(_, size, _, _, glyphs) => glyph_bounds(*size, glyphs),
        };
        let (clip, effect) = *self.stack.last().expect("balanced layers");
        // Anti-aliasing reaches the pixel an edge passes through.
        self.commands.push(Command {
            draw,
            clip,
            effect,
            bounds: bounds.inflate(1.0, 1.0),
        });
    }

    /// Opens a clip to `path`, in the chunk's coordinates.
    pub fn push_clip(&mut self, path: BezPath) {
        let (clip, effect) = *self.stack.last().expect("balanced layers");
        let id = self.trees.clips.add(Clip {
            parent: Some(clip),
            transform: self.transform,
            path,
        });
        self.made.clips.push(id);
        self.stack.push((id, effect));
    }

    /// Opens a layer with `blend`, bounded by `bounds`.
    pub fn push_layer(&mut self, blend: Blend, bounds: Option<BezPath>) {
        let (clip, effect) = *self.stack.last().expect("balanced layers");
        let id = self.trees.effects.add(Effect {
            parent: effect,
            clip,
            transform: self.transform,
            blend,
            bounds,
        });
        self.made.effects.push(id);
        self.stack.push((clip, Some(id)));
    }

    /// Closes the last layer or clip opened.
    pub fn pop(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
        }
    }

    pub fn finish(self) -> (Vec<Command>, Made) {
        (self.commands, self.made)
    }
}

/// What a glyph run can touch: its glyphs' origins widened by an em each
/// way, which holds any glyph's outline — accents, descenders and italic
/// overhang included — at that size.
fn glyph_bounds(size: f32, glyphs: &[vello_cpu::Glyph]) -> KRect {
    let mut r: Option<KRect> = None;
    for g in glyphs {
        let p = KRect::new(g.x as f64, g.y as f64, g.x as f64, g.y as f64);
        r = Some(r.map_or(p, |r| r.union(p)));
    }
    let em = size as f64;
    r.unwrap_or_default().inflate(em, em)
}

/// A command's entry in its chunk's index.
struct Local {
    at: usize,
    bounds: KRect,
}

impl RTreeObject for Local {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        aabb(self.bounds)
    }
}

fn aabb(r: KRect) -> AABB<[f64; 2]> {
    AABB::from_corners([r.x0, r.y0], [r.x1, r.y1])
}

/// The paint order of a node: its id, the path of child positions from the
/// root. A node paints before its children, and children in order.
pub type Order = Vec<u32>;

/// What a click on a node does, and where it lands: its box in the design's
/// coordinates, cut by its clipping frames.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub action: String,
    pub area: KRect,
}

/// One scene node's own commands, in its own coordinates, and their index.
pub struct Chunk {
    pub transform: Id,
    /// Where its coordinates' origin is in the design's.
    pub origin: Vec2,
    pub commands: Vec<Command>,
    index: RTree<Local>,
    /// What cuts it in the design's coordinates: its clipping frames' boxes.
    pub cut: Option<KRect>,
    /// What it covers for the drawing's area, in its own coordinates, uncut:
    /// each shape, line and blur by itself.
    pub covers: Vec<KRect>,
    /// What its commands can touch in the design's coordinates, cut —
    /// `None` when that is nothing.
    pub bounds: Option<KRect>,
    /// Its node's interaction, if it has one, in its own coordinates: the
    /// box `(0, 0, width, height)`.
    pub action: Option<(String, KRect)>,
    /// The property-tree nodes its own layers made.
    pub made: Made,
}

impl Chunk {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        transform: Id,
        origin: Vec2,
        (commands, made): (Vec<Command>, Made),
        covers: Vec<KRect>,
        action: Option<(String, KRect)>,
        cut: Option<KRect>,
    ) -> Chunk {
        let index = RTree::bulk_load(
            commands
                .iter()
                .enumerate()
                .map(|(at, c)| Local {
                    at,
                    bounds: c.bounds,
                })
                .collect(),
        );
        let mut chunk = Chunk {
            transform,
            origin,
            commands,
            index,
            cut,
            covers,
            bounds: None,
            action,
            made,
        };
        chunk.place(origin, cut);
        chunk
    }

    /// The chunk with its origin at `origin` and cut by `cut`.
    pub fn moved(mut self, origin: Vec2, cut: Option<KRect>) -> Chunk {
        self.place(origin, cut);
        self
    }

    fn place(&mut self, origin: Vec2, cut: Option<KRect>) {
        let local = self
            .commands
            .iter()
            .map(|c| c.bounds)
            .reduce(|a, b| a.union(b));
        self.bounds = local.and_then(|r| cut_to(r + origin, cut));
        self.origin = origin;
        self.cut = cut;
    }

    /// What it covers in the design's coordinates for the drawing's area:
    /// each cover cut by itself, then joined.
    pub fn cover(&self) -> Option<KRect> {
        self.covers
            .iter()
            .filter_map(|&r| cut_to(r + self.origin, self.cut))
            .reduce(|a, b| a.union(b))
    }

    /// Where its node can be clicked, in the design's coordinates.
    pub fn hit(&self) -> Option<Hit> {
        let (action, area) = self.action.as_ref()?;
        let area = *area + self.origin;
        let area = self.cut.map_or(area, |c| area.intersect(c));
        Some(Hit {
            action: action.clone(),
            area,
        })
    }

    /// Where its entry in the top-level index reaches: what it paints and
    /// where it can be clicked.
    fn reach(&self) -> Option<KRect> {
        let hit = self
            .hit()
            .map(|h| h.area)
            .filter(|r| r.width() > 0.0 && r.height() > 0.0);
        match (self.bounds, hit) {
            (Some(a), Some(b)) => Some(a.union(b)),
            (a, b) => a.or(b),
        }
    }
}

/// `r` cut by `cut`, or nothing when nothing is left.
fn cut_to(r: KRect, cut: Option<KRect>) -> Option<KRect> {
    let r = cut.map_or(r, |c| r.intersect(c));
    (r.width() > 0.0 && r.height() > 0.0).then_some(r)
}

/// A chunk's entry in the top-level index.
#[derive(Clone, PartialEq)]
struct Entry {
    order: Order,
    bounds: KRect,
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
struct Extent {
    x0: BTreeMap<i64, usize>,
    y0: BTreeMap<i64, usize>,
    x1: BTreeMap<i64, usize>,
    y1: BTreeMap<i64, usize>,
}

/// An edge as a key: to a millionth of a pixel, exactly ordered.
fn key(v: f64) -> i64 {
    (v * 1e6).round() as i64
}

impl Extent {
    fn add(&mut self, r: KRect) {
        for (m, v) in [
            (&mut self.x0, r.x0),
            (&mut self.y0, r.y0),
            (&mut self.x1, r.x1),
            (&mut self.y1, r.y1),
        ] {
            *m.entry(key(v)).or_default() += 1;
        }
    }

    fn remove(&mut self, r: KRect) {
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

    fn rect(&self) -> Option<KRect> {
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
    chunks: BTreeMap<Order, Chunk>,
    index: RTree<Entry>,
    covers: Extent,
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
            vec![0],
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
    fn found(&self, rect: KRect, pixel: f64) -> Vec<&Order> {
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

    /// The interaction of the topmost node at `(x, y)`, in the design's
    /// coordinates, that has one and whose clipping frames show it there.
    pub fn action_at(&self, x: f64, y: f64) -> Option<&str> {
        let point = KRect::new(x, y, x, y);
        self.found(point, 0.0).into_iter().rev().find_map(|order| {
            let chunk = &self.chunks[order];
            let (action, _) = chunk.action.as_ref()?;
            let a = chunk.hit()?.area;
            (x >= a.x0 && y >= a.y0 && x < a.x1 && y < a.y1).then_some(action.as_str())
        })
    }

    /// Where `transform`'s origin is in the design's coordinates.
    fn world(&self, mut transform: Id) -> Vec2 {
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
    pub fn swap_images(&mut self, last_of: &HashMap<u64, vello_cpu::peniko::ImageData>) {
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

fn frame_rect(f: Frame) -> KRect {
    KRect::new(
        f.x as f64,
        f.y as f64,
        (f.x + f.width) as f64,
        (f.y + f.height) as f64,
    )
}

/// What a backend does with the commands [`paint`] hands it, each already
/// under the transform from its coordinates to the target's pixels. A
/// backend translates each into its library's call and decides nothing.
pub trait Target {
    fn draw(&mut self, draw: &Draw, t: Affine);
    fn push_clip(&mut self, path: &BezPath, t: Affine);
    /// A layer with `blend`, bounded by `bounds` under `t` — or, when
    /// `None`, by `area`, the whole of what is painted, in the target's
    /// pixels.
    fn push_layer(&mut self, blend: Blend, bounds: Option<&BezPath>, t: Affine, area: KRect);
    fn pop(&mut self);
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Layer {
    Clip(Id),
    Effect(Id),
}

/// The engine's one paint entry point: `area` of the list — in the design's
/// coordinates — at `scale` device pixels per design pixel into `target`,
/// whose top-left is the area's. The commands that touch it, in paint order,
/// each under exactly the layers its property state needs.
pub fn paint(list: &List, area: KRect, scale: f64, target: &mut impl Target) {
    let view = Affine::scale(scale) * Affine::translate((-area.x0, -area.y0));
    let pixels = KRect::new(0.0, 0.0, area.width() * scale, area.height() * scale);
    let mut open: Vec<Layer> = Vec::new();
    let place = |transform: Id| view * Affine::translate(list.world(transform));
    for (chunk, command) in list.query(area, 1.0 / scale) {
        let want = layers(list, command.clip, command.effect);
        let keep = open.iter().zip(&want).take_while(|(a, b)| a == b).count();
        while open.len() > keep {
            open.pop();
            target.pop();
        }
        for layer in &want[keep..] {
            match *layer {
                Layer::Clip(id) => {
                    let c = list.trees.clips.get(id);
                    target.push_clip(&c.path, place(c.transform));
                }
                Layer::Effect(id) => {
                    let e = list.trees.effects.get(id);
                    target.push_layer(e.blend, e.bounds.as_ref(), place(e.transform), pixels);
                }
            }
            open.push(*layer);
        }
        target.draw(&command.draw, view * Affine::translate(chunk.origin));
    }
    for _ in open {
        target.pop();
    }
}

/// The layers a command under `clip` and `effect` is drawn in, outermost
/// first: each effect from the root down, inside its own clip, then the
/// command's clips inside the innermost effect.
fn layers(list: &List, clip: Id, effect: Option<Id>) -> Vec<Layer> {
    let mut effects = Vec::new();
    let mut at = effect;
    while let Some(e) = at {
        effects.push(e);
        at = list.trees.effects.get(e).parent;
    }
    effects.reverse();
    let mut out = Vec::new();
    let mut inside: Option<Id> = None;
    for e in effects {
        let to = list.trees.effects.get(e).clip;
        clips(list, inside, to, &mut out);
        out.push(Layer::Effect(e));
        inside = Some(to);
    }
    clips(list, inside, clip, &mut out);
    out
}

/// The clips from just below `from` down to `to`, outermost first.
fn clips(list: &List, from: Option<Id>, to: Id, out: &mut Vec<Layer>) {
    let mut chain = Vec::new();
    let mut at = Some(to);
    while let Some(c) = at {
        if Some(c) == from {
            break;
        }
        chain.push(Layer::Clip(c));
        at = list.trees.clips.get(c).parent;
    }
    out.extend(chain.into_iter().rev());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extent_is_the_least_and_greatest_edge_of_what_is_in_it() {
        let mut e = Extent::default();
        let (a, b) = (
            KRect::new(0.0, 5.0, 10.0, 20.0),
            KRect::new(-3.0, 7.0, 4.0, 30.0),
        );
        e.add(a);
        e.add(b);
        e.add(b);
        assert_eq!(e.rect(), Some(KRect::new(-3.0, 5.0, 10.0, 30.0)));
        e.remove(b);
        assert_eq!(e.rect(), Some(KRect::new(-3.0, 5.0, 10.0, 30.0)));
        e.remove(b);
        assert_eq!(e.rect(), Some(a));
        e.remove(a);
        assert_eq!(e.rect(), None);
    }
}
