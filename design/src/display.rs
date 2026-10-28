//! The display list as chunks under property trees, and the index over them:
//! what every tile, band and frame is painted from. A command doesn't nest
//! layers; it names its clip and its effect, and [`paint`] opens and closes
//! exactly the layers each command it paints needs, so any subset of the
//! list paints in order.

use std::collections::BTreeMap;

use rstar::{AABB, RTree, RTreeObject};
use vello_cpu::kurbo::{Affine, BezPath, Rect as KRect, Shape, Vec2};

use crate::render::{Draw, Frame};

/// A node in one of the property trees, by its slot.
pub type Id = u32;

/// One scene node's place: its offset from its parent's.
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
    /// Added to it, color and alpha alike.
    Add,
}

/// A layer: its blend, the bound nothing in it reaches — in its transform's
/// coordinates, or the whole area painted — and the clip it is drawn inside.
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
        self.slots[id as usize] = None;
        self.free.push(id);
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
    /// What it covers, in its chunk's coordinates.
    pub bounds: KRect,
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

/// One scene node's own commands, in its own coordinates, and their index.
pub struct Chunk {
    pub transform: Id,
    /// Where its coordinates' origin is in the design's.
    pub origin: Vec2,
    pub commands: Vec<Command>,
    index: RTree<Local>,
    /// What cuts it in the design's coordinates: its clipping frames' boxes.
    pub cut: Option<KRect>,
    /// What its commands cover in the design's coordinates, cut by its
    /// clipping frames — `None` when that is nothing.
    pub bounds: Option<KRect>,
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

/// The display list: chunks by paint order, the property trees, and the
/// index over the chunks.
pub struct List {
    pub trees: Trees,
    pub chunks: BTreeMap<Order, Chunk>,
    index: RTree<Entry>,
    /// The root clip: the drawing's own area.
    pub root_clip: Id,
    pub area: Frame,
}

impl Chunk {
    /// The chunk moved so its origin is `origin`, its cut `cut`.
    pub fn moved(mut self, origin: Vec2, cut: Option<KRect>) -> Chunk {
        let local = self
            .commands
            .iter()
            .map(|c| c.bounds)
            .reduce(|a, b| a.union(b));
        self.bounds = local.map(|r| r + origin).and_then(|r| {
            let r = cut.map_or(r, |c| r.intersect(c));
            (r.width() > 0.0 && r.height() > 0.0).then_some(r)
        });
        self.origin = origin;
        self.cut = cut;
        self
    }

    pub fn new(transform: Id, origin: Vec2, commands: Vec<Command>, cut: Option<KRect>) -> Chunk {
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
        let local = commands.iter().map(|c| c.bounds).reduce(|a, b| a.union(b));
        let bounds = local.map(|r| r + origin).and_then(|r| {
            let r = cut.map_or(r, |c| r.intersect(c));
            (r.width() > 0.0 && r.height() > 0.0).then_some(r)
        });
        Chunk {
            transform,
            origin,
            commands,
            index,
            cut,
            bounds,
        }
    }
}

impl List {
    /// A list from chunks made for a new scene: the index bulk-loaded.
    pub fn new(trees: Trees, chunks: BTreeMap<Order, Chunk>, root_clip: Id, area: Frame) -> List {
        let index = RTree::bulk_load(
            chunks
                .iter()
                .filter_map(|(order, c)| {
                    c.bounds.map(|bounds| Entry {
                        order: order.clone(),
                        bounds,
                    })
                })
                .collect(),
        );
        List {
            trees,
            chunks,
            index,
            root_clip,
            area,
        }
    }

    /// The commands that touch `rect`, in the design's coordinates, in paint
    /// order — a chunk smaller than `pixel` both ways skipped — each with its
    /// chunk.
    pub fn query(&self, rect: KRect, pixel: f64) -> Vec<(&Chunk, &Command)> {
        let mut found: Vec<&Entry> = self
            .index
            .locate_in_envelope_intersecting(aabb(rect))
            .filter(|e| e.bounds.width() >= pixel || e.bounds.height() >= pixel)
            .collect();
        found.sort_by(|a, b| a.order.cmp(&b.order));
        let mut out = Vec::new();
        for entry in found {
            let chunk = &self.chunks[&entry.order];
            let local = rect - chunk.origin;
            let mut at: Vec<usize> = chunk
                .index
                .locate_in_envelope_intersecting(aabb(local))
                .map(|l| l.at)
                .collect();
            at.sort_unstable();
            out.extend(at.into_iter().map(|i| (chunk, &chunk.commands[i])));
        }
        out
    }

    /// Takes the chunk at `order` out of the list and the index.
    pub fn take(&mut self, order: &Order) -> Option<Chunk> {
        let chunk = self.chunks.remove(order)?;
        if let Some(bounds) = chunk.bounds {
            self.index.remove(&Entry {
                order: order.clone(),
                bounds,
            });
        }
        Some(chunk)
    }

    /// Puts a chunk in the list and the index.
    pub fn put(&mut self, order: Order, chunk: Chunk) {
        if let Some(bounds) = chunk.bounds {
            self.index.insert(Entry {
                order: order.clone(),
                bounds,
            });
        }
        self.chunks.insert(order, chunk);
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
}

/// What a backend does with the commands paint hands it, each already under
/// the transform from its coordinates to the target's pixels.
pub trait Target {
    fn draw(&mut self, draw: &Draw, t: Affine);
    fn push_clip(&mut self, path: &BezPath, t: Affine);
    /// A layer with `blend`, bounded by `bounds` — or, when `None`, the area
    /// being painted, `area` in the target's pixels.
    fn push_layer(&mut self, blend: Blend, bounds: Option<&BezPath>, t: Affine, area: KRect);
    fn pop(&mut self);
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Layer {
    Clip(Id),
    Effect(Id),
}

/// Paints `area` of the list — in the design's coordinates — at `scale`
/// device pixels per design pixel into `target`, whose top-left is the
/// area's: the commands that touch it, in paint order, each under exactly
/// the layers its property state needs.
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

/// A node's own commands, nested as a shadow nests its layers, as commands
/// under property-tree nodes made for those layers inside `clip` and
/// `effect`. `glyphs` holds what each glyph run covers, in order.
pub fn flatten(
    nested: Vec<Draw>,
    glyphs: Vec<KRect>,
    transform: Id,
    clip: Id,
    effect: Option<Id>,
    trees: &mut Trees,
) -> Vec<Command> {
    let mut glyphs = glyphs.into_iter();
    let mut stack: Vec<(Id, Option<Id>)> = vec![(clip, effect)];
    let mut out = Vec::new();
    for draw in nested {
        let (clip, effect) = *stack.last().expect("balanced layers");
        let bounds = match &draw {
            Draw::Fill(path, _) => path.bounding_box(),
            Draw::Stroke(path, width, _) => {
                let half = *width as f64 / 2.0;
                path.bounding_box().inflate(half, half)
            }
            Draw::Glyphs(..) => glyphs.next().unwrap_or_default(),
            Draw::PushClip(path) => {
                let id = trees.clips.add(Clip {
                    parent: Some(clip),
                    transform,
                    path: path.clone(),
                });
                stack.push((id, effect));
                continue;
            }
            Draw::PushOpacity(o, bounds) => {
                let id = trees.effects.add(Effect {
                    parent: effect,
                    clip,
                    transform,
                    blend: Blend::Opacity(*o),
                    bounds: bounds.clone(),
                });
                stack.push((clip, Some(id)));
                continue;
            }
            Draw::PushAdd(bounds) => {
                let id = trees.effects.add(Effect {
                    parent: effect,
                    clip,
                    transform,
                    blend: Blend::Add,
                    bounds: Some(bounds.clone()),
                });
                stack.push((clip, Some(id)));
                continue;
            }
            Draw::Pop => {
                stack.pop();
                continue;
            }
        };
        // Anti-aliasing reaches the pixel an edge passes through.
        out.push(Command {
            draw,
            clip,
            effect,
            bounds: bounds.inflate(1.0, 1.0),
        });
    }
    out
}
