//! A drawing: a scene laid out and made into its display list in one
//! theme, kept so later scenes update it; its area and the images it paints.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use kurbo::{Rect as KRect, Vec2};
use peniko::ImageData;

use crate::display::{Blend, Builder, Chunk, Clip, Effect, Id, List, Order, Transform};
use crate::resolve::{self, ImageId, Node, Origin, Scene, Theme};

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
    pub(super) tree: TaffyTree<Option<Leaf>>,
    pub(super) root: NodeId,
    pub(super) theme: Theme,
    pub(super) frames: BTreeMap<Order, Frame>,
    pub(super) recs: BTreeMap<Order, Rec>,
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

    /// Every node's box, in tree order.
    pub fn frames(&self) -> Vec<Frame> {
        self.frames.values().copied().collect()
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

    /// Updates the drawing to `new` by a patch: `replaced`, the subtrees it
    /// replaced in the scene the drawing shows, each by its id and the node
    /// that was there ([`crate::wire::apply`] returns them, and
    /// [`crate::wire::replaced`] finds them between two scenes). Each is
    /// compared with what replaced it, node by node; only what differs is
    /// laid out and painted again, and nodes that only moved keep their
    /// chunks — the work is the patch's, never the scene's. The damage it
    /// returns is what changed; or why `new` can't be painted — its images
    /// too many for the GPU's atlas — in which case the drawing is `new`'s
    /// all the same.
    pub fn update(
        &mut self,
        renderer: &mut Renderer,
        new: &Scene,
        replaced: &[crate::wire::Replaced],
    ) -> Result<Damage, Undrawable> {
        renderer.update(self, new, replaced)
    }

    /// [`Drawing::update`] from `old`, the scene the drawing shows, to
    /// `new`, by the patch between them: for a caller holding both.
    pub fn update_between(
        &mut self,
        renderer: &mut Renderer,
        old: &Scene,
        new: &Scene,
    ) -> Result<Damage, Undrawable> {
        let replaced = crate::wire::replaced(&old.root, &new.root);
        renderer.update(self, new, &replaced)
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
        let mut tree: TaffyTree<Option<Leaf>> = TaffyTree::new();
        // Only the boxes read are rounded, by `placed`, as taffy would round
        // them all.
        tree.disable_rounding();
        let mut hugs = Vec::new();
        let root = build(&mut tree, &scene.root, None, &mut hugs, &scene.images);
        self.settle(&mut tree, root, &hugs);
        let mut d = Drawing {
            tree,
            root,
            theme,
            frames: BTreeMap::new(),
            recs: BTreeMap::new(),
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
            vec![0],
            root,
            t,
            Place::default(),
            c,
            None,
            None,
            false,
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
        let root = d.frames[&vec![0]];
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
    /// commands are under.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn sync(
        &mut self,
        d: &mut Drawing,
        w: &mut Walk,
        node: &Node,
        order: Order,
        id: NodeId,
        parent_t: Id,
        parent: Place,
        clip: Id,
        cut: Option<KRect>,
        effect: Option<Id>,
        remade: bool,
    ) -> bool {
        let (f, place) = placed(&d.tree, id, parent, order.len() == 1);
        let parent_origin = Vec2::new(parent.at.0 as f64, parent.at.1 as f64);
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
        // Its transform is kept, and moved: its children's are under it.
        let transform = match &old {
            Some(rec) => {
                trees.transforms.get_mut(rec.transform).offset = origin - parent_origin;
                rec.transform
            }
            None => trees.transforms.add(Transform {
                parent: Some(parent_t),
                offset: origin - parent_origin,
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
                if moved && let Some(damage) = w.damage.as_deref_mut() {
                    damage.rects.extend(c.bounds);
                }
                let c = c.moved(origin, cut);
                if moved && let Some(damage) = w.damage.as_deref_mut() {
                    damage.rects.extend(c.bounds);
                }
                (c, old.expect("kept").painted)
            }
            was => {
                if let Some(c) = &was {
                    if let Some(damage) = w.damage.as_deref_mut() {
                        damage.rects.extend(c.bounds);
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
                let c = Chunk::new(transform, origin, built, covers, action, cut);
                if let Some(damage) = w.damage.as_deref_mut() {
                    damage.rects.extend(c.bounds);
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
        d.recs.insert(
            order.clone(),
            Rec {
                transform,
                effect: own_effect,
                clip: own_clip,
                painted,
            },
        );
        match w.fresh.as_deref_mut() {
            Some(fresh) => fresh.push((order.clone(), chunk)),
            None => d.list.put(order.clone(), chunk),
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
            || is_new
            || dirty
            || resized
            || moved
            || cut_changed
            || children_remade
            || w.reflow.is_some_and(|r| r.contains(&order));
        let leaf = leaf_of(node, &w.scene.images).is_some();
        if leaf {
            return resized;
        }
        let on_path = match (all, w.path) {
            (false, Some(path)) => path.get(&order).cloned().unwrap_or_default(),
            _ => BTreeSet::new(),
        };
        let visit = |r: &mut Renderer, d: &mut Drawing, w: &mut Walk, i: usize| {
            let mut o = order.clone();
            o.push(i as u32);
            r.sync(
                d,
                w,
                &node.children[i],
                o,
                d.tree.child_at_index(id, i).expect("child"),
                transform,
                place,
                inner,
                inner_cut,
                under,
                children_remade,
            )
        };
        if all {
            for i in 0..node.children.len() {
                visit(self, d, w, i);
            }
        } else {
            let mut moves = false;
            for &i in &on_path {
                if (i as usize) < node.children.len() {
                    moves |= visit(self, d, w, i as usize);
                }
            }
            if moves {
                for i in 0..node.children.len() {
                    if !on_path.contains(&(i as u32)) {
                        visit(self, d, w, i);
                    }
                }
            }
        }
        resized
    }
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
    pub(super) path: Option<&'a HashMap<Order, BTreeSet<u32>>>,
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
