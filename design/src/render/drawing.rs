//! A drawing: a scene laid out and made into its display list in one
//! theme, kept so later scenes update it; its area and the images it paints.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use kurbo::{Rect as KRect, Vec2};
use peniko::ImageData;

use crate::display::{
    Blend, Builder, Chunk, Clip, Effect, Id, List, Order, Placed, RunId, Space, Transform,
};
use crate::resolve::{self, ImageId, Node, Origin, Scene, Theme};
use crate::tree::{NodeId as SceneId, OrderKey};

use super::*;

/// An image a drawing paints, and what it shows: an image file, known by
/// its id, or a shadow's pixels.
#[derive(Clone)]
pub(super) struct Painted {
    pub(super) data: ImageData,
    pub(super) file: Option<ImageId>,
}

impl Painted {
    /// Whether the two show the same pixels.
    pub(super) fn same(&self, other: &Painted) -> bool {
        match (self.file, other.file) {
            (Some(a), Some(b)) => a == b,
            (None, None) => {
                (self.data.width, self.data.height) == (other.data.width, other.data.height)
                    && self.data.data.data() == other.data.data.data()
            }
            _ => false,
        }
    }
}

/// A scene laid out, in one theme, and made into its display list: kept, so
/// a later scene updates it by what changed. Its layout tree, every node's
/// box by its id, the property-tree nodes each node made, the list, and the
/// images the list paints.
pub struct Drawing {
    /// The layout: the drawing's own tree and each run's.
    pub(super) laid: Laid,
    pub(super) theme: Theme,
    /// Every node's box, in its space's coordinates.
    pub(super) frames: BTreeMap<Order, Frame>,
    pub(super) recs: BTreeMap<Order, Rec>,
    /// Each run's place in the list, by its layout's slot.
    pub(super) shown: HashMap<u32, RunId>,
    /// For each run, the least start along it, in the design's coordinates,
    /// of a child an update took out: where what follows moved from.
    pub(super) shifts: HashMap<RunId, f64>,
    /// Each node's order, by its id: where a patch's nodes are in it.
    pub(super) orders: HashMap<SceneId, Order>,
    /// The display list: chunks under property trees, and their index.
    pub list: List,
    /// The blob each image file is painted from in this drawing.
    pub(super) blobs: HashMap<ImageId, ImageData>,
    /// Every image the list paints, each under a blob of its own, in the
    /// order the list first paints it: what the GPU's atlas holds for this
    /// drawing, placed as [`crate::atlas::place`] places them.
    pub images: Vec<ImageData>,
    /// What each of `images` shows.
    pub(super) shows: Vec<Painted>,
    /// The nodes whose own commands paint an image, in paint order.
    pub(super) painters: BTreeSet<Order>,
    /// Whether `images` fit the GPU's atlas together.
    pub(super) fits: Result<(), Undrawable>,
    /// How many nodes the last update visited after laying out.
    pub(super) visited: usize,
}

/// What an update to a drawing touched, in the design's coordinates: each
/// chunk it changed, where it was and where it is — and the strips the
/// drawing's area gained or lost. `all` when the area's top-left moved, which
/// moves every tile.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Damage {
    pub rects: Vec<KRect>,
    pub all: bool,
}

/// The property-tree nodes a scene node made — its transform, its opacity's
/// effect and its clip — and the images its own commands paint, in order.
#[derive(Clone)]
pub(super) struct Rec {
    pub(super) node: SceneId,
    /// The space its box and chunk are in.
    pub(super) space: Space,
    pub(super) transform: Id,
    pub(super) effect: Option<Id>,
    pub(super) clip: Option<Id>,
    pub(super) painted: Vec<(Painted, Origin)>,
}

impl Drawing {
    /// The design's drawing: its root's box plus anything painted outside
    /// it, cut by any clipping frame, in whole pixels.
    pub fn area(&self) -> Frame {
        self.list.area
    }

    /// Every node's box, in tree order, in the design's coordinates.
    pub fn frames(&self) -> Vec<Frame> {
        self.frames
            .iter()
            .map(|(o, f)| {
                let at = self.list.space_at(&self.recs[o].space);
                Frame {
                    x: f.x + at.x as f32,
                    y: f.y + at.y as f32,
                    ..*f
                }
            })
            .collect()
    }

    pub fn theme(&self) -> Theme {
        self.theme
    }

    /// The tiles `view` needs at its own zoom's level, nearest its middle
    /// first: what a compositor asks to be painted.
    pub fn view_tiles(&self, view: &crate::tiles::View) -> Vec<(u32, u32)> {
        let a = self.area();
        let area = KRect::new(
            a.x as f64,
            a.y as f64,
            (a.x + a.width) as f64,
            (a.y + a.height) as f64,
        );
        view.tiles(area, crate::tiles::Level::of(view.zoom))
    }

    /// The interaction of the topmost node at `(x, y)` — in the design's
    /// coordinates — that has one, where its clipping frames show it.
    pub fn action_at(&self, x: f32, y: f32) -> Option<&str> {
        self.list.action_at(x as f64, y as f64)
    }

    /// Updates the drawing to `new` by the patch that made it: `applied`,
    /// what [`crate::wire::apply`] did to the scene the drawing showed — the
    /// subtrees removed and placed and the nodes changed, by id. Only those
    /// are laid out and painted again, and nodes that only moved keep their
    /// chunks — the work is the patch's, never the scene's. The damage it
    /// returns is what changed; or why `new` can't be painted — its images
    /// too many for the GPU's atlas — in which case the drawing is `new`'s
    /// all the same.
    pub fn update(
        &mut self,
        renderer: &mut Renderer,
        new: &Scene,
        applied: &crate::wire::Applied,
    ) -> Result<Damage, Undrawable> {
        renderer.update(self, new, applied)
    }

    /// [`Drawing::update`] from `old`, the scene the drawing shows, to the
    /// content of `new`, by the patch between them — `old` patched to it,
    /// keeping its places, so it stays the scene the drawing shows: for a
    /// caller holding both.
    pub fn update_between(
        &mut self,
        renderer: &mut Renderer,
        old: &mut Scene,
        new: &Scene,
    ) -> Result<Damage, Undrawable> {
        let mut uses = crate::wire::Uses::of(&old.root);
        let applied = crate::wire::bring(old, &mut uses, new)
            .expect("the patch between two scenes applies to the first");
        renderer.update(self, old, &applied)
    }
}

impl Renderer {
    /// The scene laid out once and made into its display list in `theme` —
    /// every color its value there — or why it can't be drawn: images and
    /// shadows that don't fit the GPU's atlas together. What the PNG, the
    /// canvas and `devpage check` all start from.
    ///
    /// Each image is painted under a blob of its own, made in the order the
    /// drawing first paints it — except that a drawing made `after` the one
    /// a canvas last showed, painting the same images in the same order,
    /// paints that drawing's: the GPU then keeps the atlas it has, which
    /// holds exactly them, placed as a fresh one would.
    pub fn drawing(
        &mut self,
        scene: &Scene,
        theme: Theme,
        after: Option<&Drawing>,
    ) -> Result<Drawing, Undrawable> {
        self.register(&scene.fonts);
        let mut laid = Laid::build(self, scene);
        let hugs = std::mem::take(&mut laid.hugs);
        laid.settle_top(self, &hugs);
        let root = laid.root;
        let mut d = Drawing {
            laid,
            theme,
            frames: BTreeMap::new(),
            recs: BTreeMap::new(),
            shown: HashMap::new(),
            shifts: HashMap::new(),
            orders: HashMap::new(),
            list: List::new(),
            blobs: HashMap::new(),
            images: Vec::new(),
            shows: Vec::new(),
            painters: BTreeSet::new(),
            fits: Ok(()),
            visited: 0,
        };
        self.blobs.clear();
        let mut made = Vec::new();
        let (t, c) = (d.list.root_transform, d.list.root_clip);
        let mut walk = Walk {
            scene,
            dirty: &|_| true,
            reflow: None,
            path: None,
            damage: None,
            fresh: Some(&mut made),
            images: true,
        };
        self.sync(
            &mut d,
            &mut walk,
            &scene.root,
            Vec::new(),
            Owner::Top,
            root,
            t,
            Place::default(),
            (&Space::Root, Vec2::ZERO),
            c,
            None,
            None,
            (false, true),
            None,
        );
        d.list.load(made);
        d.blobs = std::mem::take(&mut self.blobs);
        self.settle_area(&mut d);
        self.settle_images(&mut d)?;
        if let Some(last) = after
            && last.shows.len() == d.shows.len()
            && last.shows.iter().zip(&d.shows).all(|(a, b)| a.same(b))
        {
            let last_of: HashMap<u64, ImageData> = d
                .shows
                .iter()
                .zip(&last.shows)
                .map(|(new, old)| (new.data.data.id(), old.data.clone()))
                .collect();
            d.list.swap_images(&last_of);
            for rec in d.recs.values_mut() {
                for (p, _) in &mut rec.painted {
                    if let Some(old) = last_of.get(&p.data.data.id()) {
                        p.data = old.clone();
                    }
                }
            }
            for data in d.blobs.values_mut() {
                if let Some(old) = last_of.get(&data.data.id()) {
                    *data = old.clone();
                }
            }
            d.shows = last.shows.clone();
            d.images = d.shows.iter().map(|p| p.data.clone()).collect();
        }
        Ok(d)
    }

    /// The drawing's area from its root's box and what its chunks cover,
    /// rounded out to whole pixels, ignoring the slivers floating point
    /// leaves on a curve's bounds.
    pub(super) fn settle_area(&mut self, d: &mut Drawing) {
        let root = d.frames[&Vec::new()];
        let mut area = frame_rect(root);
        if let Some(covered) = d.list.covered() {
            area = area.union(covered);
        }
        const SLIVER: f64 = 0.01;
        let area = KRect::new(
            (area.x0 + SLIVER).floor(),
            (area.y0 + SLIVER).floor(),
            (area.x1 - SLIVER).ceil(),
            (area.y1 - SLIVER).ceil(),
        );
        d.list.set_area(Frame {
            x: area.x0 as f32,
            y: area.y0 as f32,
            width: area.width() as f32,
            height: area.height() as f32,
        });
    }

    /// The images the drawing paints, in the order it first paints them —
    /// each image file once — and whether they fit the GPU's atlas together.
    pub(super) fn settle_images(&mut self, d: &mut Drawing) -> Result<(), Undrawable> {
        d.fits = self.fit_images(d);
        d.fits.clone()
    }

    pub(super) fn fit_images(&mut self, d: &mut Drawing) -> Result<(), Undrawable> {
        let mut files = BTreeSet::new();
        let mut painted: Vec<(Painted, Origin)> = Vec::new();
        for order in &d.painters {
            for (p, at) in &d.recs[order].painted {
                if p.file.is_none_or(|id| files.insert(id)) {
                    painted.push((p.clone(), at.clone()));
                }
            }
        }
        d.blobs.retain(|id, _| files.contains(id));
        d.shows = painted.iter().map(|(p, _)| p.clone()).collect();
        d.images = d.shows.iter().map(|p| p.data.clone()).collect();
        let sizes: Vec<(u32, u32)> = painted
            .iter()
            .map(|(p, _)| (p.data.width, p.data.height))
            .collect();
        if let Err(n) = crate::atlas::place(&sizes) {
            let (Painted { data, .. }, at) = &painted[n];
            let what = if at.file.is_empty() {
                "an image"
            } else {
                "this"
            };
            return Err(Undrawable {
                message: format!(
                    "{what} doesn't fit: the images a drawing paints — its image files, \
                     at most {side} pixels a side, and its shadows — share the GPU's one \
                     {atlas}×{atlas} atlas, and the {n} before this {w}×{h} fill it",
                    side = resolve::MAX_IMAGE_SIDE,
                    atlas = crate::atlas::SIDE,
                    w = data.width,
                    h = data.height,
                ),
                at: Some(at.clone()),
            });
        }
        Ok(())
    }

    /// Makes `node`'s chunk again when it is dirty, new or another size, or
    /// moves it when only its place changed; then its children's. `remade`
    /// says its parent's clip or effect was made anew, which its own
    /// commands are under; `all` that every node under it is to be visited.
    /// `space` is the space it is in and where that space's origin is in the
    /// design's coordinates; `item`, the run and key when it is a run's
    /// child, whose place the run's sums keep.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn sync(
        &mut self,
        d: &mut Drawing,
        w: &mut Walk,
        node: &Node,
        order: Order,
        owner: Owner,
        id: NodeId,
        parent_t: Id,
        parent: Place,
        (space, space_at): (&Space, Vec2),
        clip: Id,
        cut: Option<KRect>,
        effect: Option<Id>,
        (remade, all_below): (bool, bool),
        item: Option<(RunId, OrderKey)>,
    ) -> bool {
        let (abs, place) = placed(d.laid.tree(owner), id, parent, order.is_empty());
        let f = Frame {
            x: abs.x - space_at.x as f32,
            y: abs.y - space_at.y as f32,
            ..abs
        };
        let parent_origin = Vec2::new(
            parent.at.0 as f64 - space_at.x,
            parent.at.1 as f64 - space_at.y,
        );
        let origin = Vec2::new(f.x as f64, f.y as f64);
        d.visited += 1;
        let old = d.recs.remove(&order);
        let is_new = old.is_none();
        let was = d.frames.insert(order.clone(), f);
        let resized = was.is_none_or(|was| (was.width, was.height) != (f.width, f.height));
        let moved = was.is_none_or(|was| (was.x, was.y) != (f.x, f.y));
        let cut_changed = d.list.chunk(&order).is_none_or(|c| c.cut != cut);
        let dirty = (w.dirty)(&order);
        let remake = old.is_none() || remade || dirty || resized;
        let trees = &mut d.list.trees;
        let offset = if item.is_some() {
            Vec2::ZERO
        } else {
            origin - parent_origin
        };
        // Its transform is kept, and moved: its children's are under it.
        let transform = match &old {
            Some(rec) => {
                let t = trees.transforms.get_mut(rec.transform);
                t.offset = offset;
                t.item = item.clone();
                rec.transform
            }
            None => trees.transforms.add(Transform {
                parent: Some(parent_t),
                offset,
                item: item.clone(),
            }),
        };
        let mut children_remade = remade;
        let (own_effect, own_clip) = if remake {
            // Its opacity and its clip are changed in place where they stay,
            // so what its children paint under them still names them.
            let opacity = (node.opacity < 1.0).then_some(node.opacity);
            let had = old.as_ref().and_then(|r| r.effect);
            let new_effect = match (had, opacity) {
                (Some(e), Some(o)) if !remade => {
                    trees.effects.get_mut(e).blend = Blend::Opacity(o);
                    Some(e)
                }
                (had, o) => {
                    if let Some(e) = had {
                        trees.effects.remove(e);
                    }
                    children_remade |= had.is_some() || o.is_some();
                    o.map(|o| {
                        trees.effects.add(Effect {
                            parent: effect,
                            clip,
                            transform,
                            blend: Blend::Opacity(o),
                            bounds: None,
                        })
                    })
                }
            };
            let shape = node.clip.then(|| {
                node_shape(
                    node,
                    KRect::new(0.0, 0.0, f.width as f64, f.height as f64),
                    0.0,
                )
            });
            let had = old.as_ref().and_then(|r| r.clip);
            let new_clip = match (had, shape) {
                (Some(c), Some(path)) if !remade => {
                    trees.clips.get_mut(c).path = path;
                    Some(c)
                }
                (had, shape) => {
                    if let Some(c) = had {
                        trees.clips.remove(c);
                    }
                    children_remade |= had.is_some() || shape.is_some();
                    shape.map(|path| {
                        trees.clips.add(Clip {
                            parent: Some(clip),
                            transform,
                            path,
                        })
                    })
                }
            };
            (new_effect, new_clip)
        } else {
            let rec = old.as_ref().expect("kept");
            (rec.effect, rec.clip)
        };
        let under = own_effect.or(effect);
        let was = d.list.take(&order);
        let (chunk, painted) = match was {
            Some(c) if !remake => {
                let moved = c.origin != origin || c.cut != cut;
                let at = d.list.space_at(space);
                if moved && let Some(damage) = w.damage.as_deref_mut() {
                    damage.rects.extend(c.bounds.map(|b| b + at));
                }
                let c = c.moved(origin, cut);
                if moved && let Some(damage) = w.damage.as_deref_mut() {
                    damage.rects.extend(c.bounds.map(|b| b + space_at));
                }
                (c, old.expect("kept").painted)
            }
            was => {
                if let Some(c) = &was {
                    if let Some(damage) = w.damage.as_deref_mut() {
                        let at = d.list.space_at(&c.space);
                        damage.rects.extend(c.bounds.map(|b| b + at));
                    }
                    d.list.free(&c.made);
                }
                let local = Frame {
                    x: 0.0,
                    y: 0.0,
                    width: f.width,
                    height: f.height,
                };
                self.painted.clear();
                let mut covers = Vec::new();
                let mut b = Builder::new(&mut d.list.trees, transform, clip, under);
                self.own(w.scene, node, local, d.theme, &mut b, &mut covers);
                let mut built = b.finish();
                let mut painted = std::mem::take(&mut self.painted);
                // A shadow that paints the very pixels it painted before
                // keeps its blobs, so the GPU keeps its atlas.
                if let Some(rec) = &old
                    && rec.painted.len() == painted.len()
                    && rec
                        .painted
                        .iter()
                        .zip(&painted)
                        .all(|(a, b)| a.0.same(&b.0))
                {
                    let last_of: HashMap<u64, ImageData> = painted
                        .iter()
                        .zip(&rec.painted)
                        .map(|(new, old)| (new.0.data.data.id(), old.0.data.clone()))
                        .collect();
                    swap(&mut built.0, &last_of);
                    painted = rec.painted.clone();
                }
                let action = node
                    .on_click
                    .clone()
                    .map(|a| (a, KRect::new(0.0, 0.0, f.width as f64, f.height as f64)));
                let c = Chunk::new(transform, space.clone(), origin, built, covers, action, cut);
                if let Some(damage) = w.damage.as_deref_mut() {
                    damage.rects.extend(c.bounds.map(|b| b + space_at));
                }
                if !painted.is_empty() || old.as_ref().is_some_and(|r| !r.painted.is_empty()) {
                    w.images = true;
                }
                (c, painted)
            }
        };
        if painted.is_empty() {
            d.painters.remove(&order);
        } else {
            d.painters.insert(order.clone());
        }
        d.orders.insert(node.id.clone(), order.clone());
        d.recs.insert(
            order.clone(),
            Rec {
                node: node.id.clone(),
                space: space.clone(),
                transform,
                effect: own_effect,
                clip: own_clip,
                painted,
            },
        );
        match w.fresh.as_deref_mut() {
            Some(fresh) if *space == Space::Root => fresh.push((order.clone(), chunk)),
            _ => d.list.put(order.clone(), chunk),
        }
        let rect = frame_rect(f);
        let (inner, inner_cut) = match own_clip {
            Some(c) => (c, Some(cut.map_or(rect, |k| rect.intersect(k)))),
            None => (clip, cut),
        };
        // Its children: all of them when what they are placed by or drawn
        // under changed; otherwise only those on a changed node's path, and
        // the rest too when one of those changed size, which can move them.
        let all = w.path.is_none()
            || all_below
            || is_new
            || dirty
            || resized
            || moved
            || cut_changed
            || children_remade
            || w.reflow.is_some_and(|r| r.contains(&order));
        if leaf_of(node, &w.scene.images).is_some() {
            return resized;
        }
        let on_path = match (all, w.path) {
            (false, Some(path)) => path.get(&order).cloned().unwrap_or_default(),
            _ => BTreeSet::new(),
        };
        let down = Down {
            transform,
            place,
            space,
            space_at,
            clip: inner,
            cut: inner_cut,
            effect: under,
            remade: children_remade,
        };
        if let Some(&rid) = d.laid.run_of.get(&order) {
            // A run's children are placed by its sums: its own move or change
            // of size moves them without a visit — what does need one, a
            // move by a fraction of a pixel, a new breadth or a new padding,
            // the run finds itself.
            let every = w.path.is_none() || all_below || is_new || dirty || children_remade;
            let on_path = match w.path {
                Some(path) if !every => path.get(&order).cloned().unwrap_or_default(),
                _ => BTreeSet::new(),
            };
            self.sync_run(
                d, w, node, &order, owner, id, rid, &down, every, &on_path, is_new,
            );
            return resized;
        }
        let visit = |r: &mut Renderer, d: &mut Drawing, w: &mut Walk, k: &OrderKey, all: bool| {
            let Some(child) = node.children.get(k) else {
                return false;
            };
            let mut o = order.clone();
            o.push(k.clone());
            let (owner, id) = d.laid.nodes[&o];
            r.sync(
                d,
                w,
                child,
                o,
                owner,
                id,
                down.transform,
                down.place,
                (down.space, down.space_at),
                down.clip,
                down.cut,
                down.effect,
                (down.remade, all),
                None,
            )
        };
        // Only in auto layout does one child's size move its siblings.
        let flows = node.layout != resolve::Layout::None;
        if all {
            for k in node.children.entries().map(|(k, _)| k) {
                visit(self, d, w, k, all_below);
            }
        } else {
            let mut moves = false;
            for k in &on_path {
                moves |= visit(self, d, w, k, false);
            }
            if moves && flows {
                for k in node.children.entries().map(|(k, _)| k) {
                    if !on_path.contains(k) {
                        visit(self, d, w, k, false);
                    }
                }
            }
        }
        resized
    }

    /// A run's children: each placed by the run's sums, those `all` or on
    /// the path visited, and — when one of them changed size by a fraction
    /// of a pixel, which moves where every child after it rounds — every
    /// child after it too. A child that changed size moves the ones after it
    /// by the sums alone; their damage is one rectangle, from where the
    /// first change starts to where the run ends.
    #[allow(clippy::too_many_arguments)]
    fn sync_run(
        &mut self,
        d: &mut Drawing,
        w: &mut Walk,
        node: &Node,
        order: &Order,
        owner: Owner,
        id: NodeId,
        rid: u32,
        down: &Down,
        all: bool,
        on_path: &BTreeSet<OrderKey>,
        is_new: bool,
    ) {
        let size = d.laid.tree(owner).unrounded_layout(id).size;
        let Some((run, asked, lead)) = d.laid.run_places(rid, size) else {
            return;
        };
        let (pc0, _) = run.padding_across();
        let (row, gap, line) = (run.row, run.gap as f64, asked.line);
        let drid = match d.shown.get(&rid) {
            Some(&r) => r,
            None => {
                let r = d.list.add_run(down.space.clone());
                d.shown.insert(rid, r);
                r
            }
        };
        let raw = Vec2::new(
            down.place.raw.0 as f64 - down.space_at.x,
            down.place.raw.1 as f64 - down.space_at.y,
        );
        let at = Vec2::new(
            down.place.at.0 as f64 - down.space_at.x,
            down.place.at.1 as f64 - down.space_at.y,
        );
        let fraction = |v: Vec2| Vec2::new(v.x - v.x.floor(), v.y - v.y.floor());
        let old_entry = d.list.run(drid).entry().map(|e| e + down.space_at);
        let changed = {
            let dr = d.list.run(drid);
            is_new
                || dr.row != row
                || dr.start != lead as f64
                || dr.across != pc0 as f64
                || dr.gap != gap
                || fraction(dr.raw) != fraction(raw)
                || dr.line != line as f64
        };
        {
            let dr = d.list.run_mut(drid);
            dr.raw = raw;
            dr.at = at;
            dr.row = row;
            dr.start = lead as f64;
            dr.across = pc0 as f64;
            dr.gap = gap;
            dr.line = line as f64;
            dr.cut = down.cut;
        }
        let every = all || changed;
        let mut shift: Option<f64> = d.shifts.remove(&drid);
        let mut after = false;
        let keys: Vec<OrderKey> = if every {
            node.children.entries().map(|(k, _)| k.clone()).collect()
        } else {
            on_path.iter().cloned().collect()
        };
        let mut queue: std::collections::VecDeque<OrderKey> = keys.into();
        let mut seen = BTreeSet::new();
        while let Some(k) = queue.pop_front() {
            if !seen.insert(k.clone()) {
                continue;
            }
            let Some(child) = node.children.get(&k) else {
                continue;
            };
            let (run, asked, _) = d.laid.run_places(rid, size).expect("placed");
            let Some(s) = asked.sizes.get(&k).copied() else {
                continue;
            };
            let stretch = run.children[&k].stretch;
            let offset = run.across(line, &s, stretch) as f64;
            let child_node = run.children[&k].node;
            self.relay(&mut d.laid.runs, rid, &k, line);
            let (before, old_size) = {
                let dr = d.list.run(drid);
                let before = dr.prefix(&k);
                (before, dr.child(&k).map(|(_, p)| p.size))
            };
            let item = child_place(down.place, row, lead as f64 + before, pc0 as f64 + offset);
            let item_at = Vec2::new(item.at.0 as f64, item.at.1 as f64);
            let mut o = order.clone();
            o.push(k.clone());
            self.sync(
                d,
                w,
                child,
                o,
                Owner::Run(rid),
                child_node,
                down.transform,
                item,
                (&Space::Item(drid, k.clone()), item_at),
                down.clip,
                None,
                down.effect,
                (down.remade, every || after),
                Some((drid, k.clone())),
            );
            let delta = Vec2::new(
                (item.at.0 - item.raw.0) as f64,
                (item.at.1 - item.raw.1) as f64,
            );
            d.list.place_child(
                drid,
                k.clone(),
                Some(Placed::new(s.along as f64, gap, offset, delta)),
            );
            let grew = old_size.map_or(s.along as f64 + gap, |o| s.along as f64 - o);
            if grew != 0.0 {
                let start = d.list.space_at(down.space) + d.list.run(drid).place(before, offset).1;
                let start = if row { start.x } else { start.y };
                shift = Some(shift.map_or(start, |s: f64| s.min(start)));
                if !every && !after && (grew - grew.round()).abs() > 1e-4 {
                    // Every child after it rounds where it now falls.
                    after = true;
                    let later: Vec<OrderKey> = d
                        .list
                        .run(drid)
                        .keys_after(&k)
                        .into_iter()
                        .filter(|l| !seen.contains(l))
                        .collect();
                    queue.extend(later);
                }
            }
        }
        d.list.flush();
        if let Some(start) = shift
            && let Some(damage) = w.damage.as_deref_mut()
        {
            let new_entry = d.list.run(drid).entry().map(|e| e + down.space_at);
            if let Some(both) = match (old_entry, new_entry) {
                (Some(a), Some(b)) => Some(a.union(b)),
                (a, b) => a.or(b),
            } {
                let r = if row {
                    KRect::new(start.max(both.x0), both.y0, both.x1, both.y1)
                } else {
                    KRect::new(both.x0, start.max(both.y0), both.x1, both.y1)
                };
                if r.width() > 0.0 && r.height() > 0.0 {
                    damage.rects.push(r);
                }
            }
        }
    }
}

/// What a node's children are visited under.
pub(super) struct Down<'a> {
    pub(super) transform: Id,
    pub(super) place: Place,
    pub(super) space: &'a Space,
    pub(super) space_at: Vec2,
    pub(super) clip: Id,
    pub(super) cut: Option<KRect>,
    pub(super) effect: Option<Id>,
    pub(super) remade: bool,
}

/// What a [`Renderer::sync`] walk reads and adds to.
pub(super) struct Walk<'a> {
    pub(super) scene: &'a Scene,
    /// Whether a node's own attributes changed.
    pub(super) dirty: &'a dyn Fn(&Order) -> bool,
    /// The nodes whose children were added or removed: all their children
    /// may have moved.
    pub(super) reflow: Option<&'a BTreeSet<Order>>,
    /// For an update, the children of each node on a changed node's path
    /// that lead to one; `None` walks every node.
    pub(super) path: Option<&'a HashMap<Order, BTreeSet<OrderKey>>>,
    pub(super) damage: Option<&'a mut Damage>,
    /// For a new drawing, the chunks made, to bulk-load the index with.
    pub(super) fresh: Option<&'a mut Vec<(Order, Chunk)>>,
    /// Whether a chunk made again paints an image, or painted one.
    pub(super) images: bool,
}

/// The commands' images swapped for the blobs `last_of` names.
pub(super) fn swap(commands: &mut [crate::display::Command], last_of: &HashMap<u64, ImageData>) {
    for c in commands {
        if let Draw::Fill(_, Paint::Image(data, _)) = &mut c.draw
            && let Some(old) = last_of.get(&data.data.id())
        {
            *data = old.clone();
        }
    }
}

#[cfg(test)]
mod tests;
