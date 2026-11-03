//! A chunk: one scene node's own commands, in its own coordinates, under
//! property-tree nodes, with an R-tree over them; and the builder that makes
//! them, opening a node's layers as property-tree nodes.

use kurbo::{BezPath, Rect as KRect, Shape, Vec2};
use rstar::{AABB, RTree, RTreeObject};

use super::Draw;

use super::*;

/// A drawing command and the property state it is drawn under: its clip and
/// its effect, if any. Its transform is its chunk's.
pub struct Command {
    /// What it paints, in its chunk's coordinates.
    pub draw: Draw,
    /// The clip node it is drawn inside.
    pub clip: Id,
    /// The effect node whose layer it is drawn in; `None` when it is in none.
    pub effect: Option<Id>,
    /// Every pixel it can touch, in its chunk's coordinates.
    pub bounds: KRect,
}

/// Builds one chunk's commands, turning the layers they are drawn in into
/// property-tree nodes under the chunk's transform.
pub struct Builder<'a> {
    /// The property trees its layers' clip and effect nodes are added to.
    pub(super) trees: &'a mut Trees,
    /// The chunk's transform node, which every clip and effect it opens is
    /// placed by.
    pub(super) transform: Id,
    /// The clip and effect a command is drawn under, innermost last; the first
    /// is the chunk's own and is never popped.
    pub(super) stack: Vec<(Id, Option<Id>)>,
    /// The commands drawn so far, in paint order.
    pub(super) commands: Vec<Command>,
    /// The property-tree nodes made for layers, to be freed with the chunk.
    pub(super) made: Made,
}

/// The property-tree nodes a chunk's own layers made.
#[derive(Default, Clone)]
pub struct Made {
    /// The clip nodes, in the order they were opened.
    pub clips: Vec<Id>,
    /// The effect nodes, in the order they were opened.
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

    /// The commands drawn, in paint order, and the property-tree nodes their
    /// layers made.
    pub fn finish(self) -> (Vec<Command>, Made) {
        (self.commands, self.made)
    }
}

/// What a glyph run can touch: its glyphs' origins widened by an em each
/// way, which holds any glyph's outline — accents, descenders and italic
/// overhang included — at that size.
pub(super) fn glyph_bounds(size: f32, glyphs: &[super::Glyph]) -> KRect {
    let mut r: Option<KRect> = None;
    for g in glyphs {
        let p = KRect::new(g.x as f64, g.y as f64, g.x as f64, g.y as f64);
        r = Some(r.map_or(p, |r| r.union(p)));
    }
    let em = size as f64;
    r.unwrap_or_default().inflate(em, em)
}

/// A command's entry in its chunk's index.
pub(super) struct Local {
    /// The command's place in its chunk's `commands`.
    pub(super) at: usize,
    /// The command's bounds, in its chunk's coordinates.
    pub(super) bounds: KRect,
}

impl RTreeObject for Local {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        aabb(self.bounds)
    }
}

/// `r` as an R-tree envelope.
pub(super) fn aabb(r: KRect) -> AABB<[f64; 2]> {
    AABB::from_corners([r.x0, r.y0], [r.x1, r.y1])
}

/// A node's paint order: the order keys from the root down to it. A node
/// paints before its children, and children in order.
pub use crate::tree::Order;

/// What a click on a node does, and where it lands: its box in the design's
/// coordinates, cut by its clipping frames.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    /// The action the node's `on_click` names.
    pub action: String,
    /// Where the click lands, in the design's coordinates.
    pub area: KRect,
}

/// One scene node's own commands, in its own coordinates, and their index.
pub struct Chunk {
    /// Its node's transform node, which every clip and effect it opens is
    /// under.
    pub transform: Id,
    /// The space its coordinates are in: the design's, or a run child's.
    pub space: Space,
    /// Where its coordinates' origin is in its space's.
    pub origin: Vec2,
    /// Its node's own commands, in paint order and in its own coordinates.
    pub commands: Vec<Command>,
    /// The R-tree over `commands`' bounds.
    pub(super) index: RTree<Local>,
    /// What cuts it in its space's coordinates: its clipping frames' boxes,
    /// those inside its space.
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
    /// The chunk of `commands`, placed at `origin` in `space` and cut by
    /// `cut`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        transform: Id,
        space: Space,
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
            space,
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

    /// Moves the chunk's origin to `origin` and its cut to `cut`, and works
    /// out its `bounds` again.
    pub(super) fn place(&mut self, origin: Vec2, cut: Option<KRect>) {
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

    /// Where its entry in the top-level index reaches: what it paints and
    /// where it can be clicked.
    pub(super) fn reach(&self) -> Option<KRect> {
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
pub(super) fn cut_to(r: KRect, cut: Option<KRect>) -> Option<KRect> {
    let r = cut.map_or(r, |c| r.intersect(c));
    (r.width() > 0.0 && r.height() > 0.0).then_some(r)
}
