//! An update: a drawing brought from one scene to the next by what changed
//! between them, node by node, and the damage it leaves.

use std::collections::{BTreeSet, HashMap};

use vello_cpu::kurbo::Rect as KRect;

use crate::display::Order;
use crate::resolve::{Node, Scene};

use super::*;
impl Renderer {
    pub(super) fn update(
        &mut self,
        d: &mut Drawing,
        old: &Scene,
        new: &Scene,
    ) -> Result<Damage, Undrawable> {
        self.register(&new.fonts);
        d.visited = 0;
        let mut changes = Changes::default();
        changes.diff(&old.root, &new.root, vec![0], false);
        let mut damage = Damage::default();
        let mut images = false;
        // What is gone: the children past a node's new count, and the
        // subtrees made again from nothing — their chunks, records and
        // property-tree nodes, found by their ids' prefix.
        for order in changes.removed.iter().chain(&changes.rebuilt) {
            images |= d.forget(order, &mut damage);
        }
        // The layout tree, edited where the scene changed. Children go from
        // the end of their parent's list, so every other id still names its
        // node.
        let mut hugs: Vec<(Order, NodeId, bool, bool)> = Vec::new();
        for order in changes.removed.iter().rev() {
            let at = &order[..order.len() - 1];
            let parent = taffy_at(&d.tree, d.root, &at[1..]);
            let gone = d
                .tree
                .remove_child_at_index(parent, order[order.len() - 1] as usize)
                .expect("child");
            remove_subtree(&mut d.tree, gone);
            if d.tree.child_count(parent) == 0 {
                // A frame left with no children is laid out as a leaf, which
                // never clears the list of absolutely placed children taffy
                // kept for it — the ones just removed: it is made afresh.
                let node = find_node(&new.root, at);
                let up = (at.len() > 1).then(|| find_node(&new.root, &at[..at.len() - 1]));
                let fresh = d.tree.new_leaf(style_of(node, up)).expect("node");
                if at.len() == 1 {
                    d.root = fresh;
                } else {
                    let grand = taffy_at(&d.tree, d.root, &at[1..at.len() - 1]);
                    d.tree
                        .replace_child_at_index(grand, at[at.len() - 1] as usize, fresh)
                        .expect("child");
                }
                let _ = d.tree.remove(parent);
            }
        }
        for order in changes.rebuilt.iter().chain(&changes.added) {
            let node = find_node(&new.root, order);
            let parent = (order.len() > 1).then(|| find_node(&new.root, &order[..order.len() - 1]));
            let mut made = Vec::new();
            let id = build(&mut d.tree, node, parent, &mut made, &new.images);
            hugs.extend(made.into_iter().map(|(id, w, h)| (order.clone(), id, w, h)));
            if order.len() == 1 {
                let gone = std::mem::replace(&mut d.root, id);
                remove_subtree(&mut d.tree, gone);
                continue;
            }
            let parent_id = taffy_at(&d.tree, d.root, &order[1..order.len() - 1]);
            let at = order[order.len() - 1] as usize;
            if at < d.tree.child_count(parent_id) {
                let gone = d
                    .tree
                    .replace_child_at_index(parent_id, at, id)
                    .expect("child");
                remove_subtree(&mut d.tree, gone);
            } else {
                d.tree.add_child(parent_id, id).expect("child");
            }
        }
        for order in &changes.attrs {
            let node = find_node(&new.root, order);
            let parent = (order.len() > 1).then(|| find_node(&new.root, &order[..order.len() - 1]));
            let id = taffy_at(&d.tree, d.root, &order[1..]);
            d.tree.set_style(id, style_of(node, parent)).expect("style");
            if let Some(leaf) = leaf_of(node, &new.images) {
                d.tree.set_node_context(id, Some(Some(leaf))).expect("leaf");
            }
        }
        // The frames that hug their children and lie on a changed node's
        // path: only their extent can have changed. Innermost first.
        let mut seen = BTreeSet::new();
        for order in changes.touched() {
            for k in (1..=order.len()).rev() {
                let at = &order[..k];
                if !seen.insert(at.to_vec()) {
                    continue;
                }
                let node = find_node(&new.root, at);
                let parent = (k > 1).then(|| find_node(&new.root, &order[..k - 1]));
                if let Some((w, h)) = hugs_of(node, parent) {
                    hugs.push((at.to_vec(), taffy_at(&d.tree, d.root, &at[1..]), w, h));
                }
            }
        }
        hugs.sort_by_key(|h| std::cmp::Reverse(h.0.len()));
        let mut once = std::collections::HashSet::new();
        hugs.retain(|h| once.insert(h.1));
        let hugs: Vec<(NodeId, bool, bool)> =
            hugs.into_iter().map(|(_, id, w, h)| (id, w, h)).collect();
        // Each starts from no size, as in a fresh layout, so it settles where
        // a fresh one does.
        for &(id, w, h) in &hugs {
            let mut style = d.tree.style(id).expect("style").clone();
            if w {
                style.size.width = Dimension::auto();
            }
            if h {
                style.size.height = Dimension::auto();
            }
            d.tree.set_style(id, style).expect("style");
        }
        let root = d.root;
        self.settle(&mut d.tree, root, &hugs);
        // Only the changed nodes' paths are walked, and below them only
        // what moved, changed size or was made again.
        let mut path: HashMap<Order, BTreeSet<u32>> = HashMap::new();
        for order in changes.touched() {
            for k in 1..order.len() {
                path.entry(order[..k].to_vec())
                    .or_default()
                    .insert(order[k]);
            }
        }
        let attrs: BTreeSet<Order> = changes.attrs.iter().cloned().collect();
        let dirty = |o: &Order| attrs.contains(o);
        self.blobs = std::mem::take(&mut d.blobs);
        let (t, c) = (d.list.root_transform, d.list.root_clip);
        let reflow: BTreeSet<Order> = changes
            .added
            .iter()
            .chain(&changes.removed)
            .map(|o| o[..o.len() - 1].to_vec())
            .collect();
        let mut walk = Walk {
            scene: new,
            dirty: &dirty,
            reflow: Some(&reflow),
            path: Some(&path),
            damage: Some(&mut damage),
            fresh: None,
            images: false,
        };
        let root = d.root;
        self.sync(
            d,
            &mut walk,
            &new.root,
            vec![0],
            root,
            t,
            (0.0, 0.0),
            c,
            None,
            None,
            false,
        );
        images |= walk.images;
        d.blobs = std::mem::take(&mut self.blobs);
        let area = d.list.area;
        self.settle_area(d);
        let now = d.list.area;
        if now != area {
            if (now.x, now.y) != (area.x, area.y) {
                damage.all = true;
            } else {
                // What the area gained or lost along its right and bottom.
                let (w0, w1) = (area.width.min(now.width), area.width.max(now.width));
                let (h0, h1) = (area.height.min(now.height), area.height.max(now.height));
                let (x, y) = (now.x as f64, now.y as f64);
                if w0 < w1 {
                    damage
                        .rects
                        .push(KRect::new(x + w0 as f64, y, x + w1 as f64, y + h1 as f64));
                }
                if h0 < h1 {
                    damage
                        .rects
                        .push(KRect::new(x, y + h0 as f64, x + w1 as f64, y + h1 as f64));
                }
            }
        }
        if images {
            self.settle_images(d)?;
        } else {
            d.fits.clone()?;
        }
        Ok(damage)
    }
}

/// What changed between two scenes, node by node, by id: nodes whose own
/// attributes changed, nodes made again from nothing — a leaf of the layout
/// tree that became a frame or the other way round — and children added past
/// or removed from the end of a node's list.
#[derive(Default, Debug)]
pub(super) struct Changes {
    pub(super) attrs: Vec<Order>,
    pub(super) rebuilt: Vec<Order>,
    pub(super) added: Vec<Order>,
    pub(super) removed: Vec<Order>,
}

impl Changes {
    /// The changes from `old` to `new` at `order`; `restyle` when its
    /// parent's layout changed, which decides part of its style.
    pub(super) fn diff(&mut self, old: &Node, new: &Node, order: Order, restyle: bool) {
        if old == new && !restyle {
            return;
        }
        if leafy(old) != leafy(new) {
            self.rebuilt.push(order);
            return;
        }
        if restyle || !same_own(old, new) {
            self.attrs.push(order.clone());
        }
        let common = old.children.len().min(new.children.len());
        let layout = old.layout != new.layout;
        for i in 0..common {
            let mut o = order.clone();
            o.push(i as u32);
            self.diff(&old.children[i], &new.children[i], o, layout);
        }
        for i in common..new.children.len() {
            let mut o = order.clone();
            o.push(i as u32);
            self.added.push(o);
        }
        for i in common..old.children.len() {
            let mut o = order.clone();
            o.push(i as u32);
            self.removed.push(o);
        }
    }

    /// The nodes of the new scene a change is at: every changed node, and
    /// the parent of each removed one.
    pub(super) fn touched(&self) -> impl Iterator<Item = &[u32]> {
        self.attrs
            .iter()
            .chain(&self.rebuilt)
            .chain(&self.added)
            .map(|o| o.as_slice())
            .chain(self.removed.iter().map(|o| &o[..o.len() - 1]))
    }
}

/// Whether two nodes' own attributes are the same, their children aside.
pub(super) fn same_own(a: &Node, b: &Node) -> bool {
    a.kind == b.kind
        && a.name == b.name
        && a.x == b.x
        && a.y == b.y
        && a.width == b.width
        && a.height == b.height
        && a.opacity == b.opacity
        && a.on_click == b.on_click
        && a.fill == b.fill
        && a.stroke == b.stroke
        && a.stroke_width == b.stroke_width
        && a.radius == b.radius
        && a.shadow == b.shadow
        && a.clip == b.clip
        && a.layout == b.layout
        && a.gap == b.gap
        && a.padding == b.padding
        && a.align == b.align
        && a.justify == b.justify
        && a.text == b.text
        && a.vector == b.vector
        && a.image == b.image
}

impl Drawing {
    /// Takes out everything at and under `order` — chunks, records, frames
    /// and the property-tree nodes they made — adding where the chunks were
    /// to `damage`. True when any of them painted an image.
    pub(super) fn forget(&mut self, order: &Order, damage: &mut Damage) -> bool {
        let under: Vec<Order> = self
            .recs
            .range(order.clone()..)
            .take_while(|(o, _)| o.starts_with(order))
            .map(|(o, _)| o.clone())
            .collect();
        let mut images = false;
        for o in under {
            if let Some(c) = self.list.take(&o) {
                damage.rects.extend(c.bounds);
                self.list.free(&c.made);
            }
            if let Some(rec) = self.recs.remove(&o) {
                images |= !rec.painted.is_empty();
                self.painters.remove(&o);
                self.list.trees.transforms.remove(rec.transform);
                if let Some(e) = rec.effect {
                    self.list.trees.effects.remove(e);
                }
                if let Some(c) = rec.clip {
                    self.list.trees.clips.remove(c);
                }
            }
            self.frames.remove(&o);
        }
        images
    }
}

#[cfg(test)]
mod tests;
