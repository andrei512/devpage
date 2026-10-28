//! An update: a drawing brought from one scene to the next by what changed
//! between them, node by node, and the damage it leaves.

use std::collections::{BTreeSet, HashMap};

use kurbo::Rect as KRect;

use crate::display::Order;
use crate::resolve::Scene;
use crate::tree::OrderKey;

use super::*;
impl Renderer {
    pub(super) fn update(
        &mut self,
        d: &mut Drawing,
        new: &Scene,
        applied: &crate::wire::Applied,
    ) -> Result<Damage, Undrawable> {
        self.register(&new.fonts);
        d.visited = 0;
        let changes = Changes::of(d, new, applied);
        let mut damage = Damage::default();
        let mut images = false;
        // What is gone: subtrees removed, moved away or made again — their
        // layout nodes, chunks, records and property-tree nodes.
        for order in &changes.removed {
            let gone = d.nodes[order];
            images |= d.forget(order, &mut damage);
            if order.is_empty() {
                continue;
            }
            let at = &order[..order.len() - 1];
            let parent = d.nodes[at];
            d.tree.remove_child(parent, gone).expect("child");
            remove_subtree(&mut d.tree, gone);
            if d.tree.child_count(parent) == 0 {
                // A frame left with no children is laid out as a leaf, which
                // never clears the list of absolutely placed children taffy
                // kept for it — the ones just removed: it is made afresh.
                let node = find_node(&new.root, at);
                let up = (!at.is_empty()).then(|| find_node(&new.root, &at[..at.len() - 1]));
                let fresh = d.tree.new_leaf(style_of(node, up)).expect("node");
                if at.is_empty() {
                    d.root = fresh;
                } else {
                    let grand = d.nodes[&at[..at.len() - 1]];
                    let i = d
                        .tree
                        .children(grand)
                        .expect("children")
                        .iter()
                        .position(|&c| c == parent)
                        .expect("a child of its parent");
                    d.tree
                        .replace_child_at_index(grand, i, fresh)
                        .expect("child");
                }
                d.nodes.insert(at.to_vec(), fresh);
                let _ = d.tree.remove(parent);
            }
        }
        // What is placed, parents before children and siblings in order, so
        // the siblings before each are in the layout tree already.
        let mut hugs: Vec<(Order, NodeId, bool, bool)> = Vec::new();
        for order in &changes.added {
            let node = find_node(&new.root, order);
            let parent =
                (!order.is_empty()).then(|| find_node(&new.root, &order[..order.len() - 1]));
            let mut made = Vec::new();
            let mut hug = Vec::new();
            let id = build(
                &mut d.tree,
                node,
                parent,
                &mut hug,
                &new.images,
                order.clone(),
                &mut made,
            );
            d.nodes.extend(made);
            hugs.extend(hug.into_iter().map(|(id, w, h)| (order.clone(), id, w, h)));
            if order.is_empty() {
                let gone = std::mem::replace(&mut d.root, id);
                remove_subtree(&mut d.tree, gone);
                continue;
            }
            let at = &order[..order.len() - 1];
            let parent_id = d.nodes[at];
            let key = &order[order.len() - 1];
            let place = parent.expect("a parent").children.rank(key);
            d.tree
                .insert_child_at_index(parent_id, place, id)
                .expect("child");
        }
        for order in &changes.attrs {
            let node = find_node(&new.root, order);
            let parent =
                (!order.is_empty()).then(|| find_node(&new.root, &order[..order.len() - 1]));
            let id = d.nodes[order];
            d.tree.set_style(id, style_of(node, parent)).expect("style");
            if let Some(leaf) = leaf_of(node, &new.images) {
                d.tree.set_node_context(id, Some(Some(leaf))).expect("leaf");
            }
        }
        // The frames that hug their children and lie on a changed node's
        // path: only their extent can have changed. Innermost first.
        let mut seen = BTreeSet::new();
        for order in changes.touched() {
            for k in (0..=order.len()).rev() {
                let at = &order[..k];
                if !seen.insert(at.to_vec()) {
                    continue;
                }
                let node = find_node(&new.root, at);
                let parent = (k > 0).then(|| find_node(&new.root, &order[..k - 1]));
                if let Some((w, h)) = hugs_of(node, parent) {
                    hugs.push((at.to_vec(), d.nodes[at], w, h));
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
        let mut path: HashMap<Order, BTreeSet<OrderKey>> = HashMap::new();
        for order in changes.touched() {
            for k in 0..order.len() {
                path.entry(order[..k].to_vec())
                    .or_default()
                    .insert(order[k].clone());
            }
        }
        let attrs: BTreeSet<Order> = changes.attrs.iter().cloned().collect();
        let dirty = |o: &Order| attrs.contains(o);
        self.blobs = std::mem::take(&mut d.blobs);
        let (t, c) = (d.list.root_transform, d.list.root_clip);
        // A parent in auto layout whose children came or went: they may all
        // have moved.
        let reflow: BTreeSet<Order> = changes
            .added
            .iter()
            .chain(&changes.removed)
            .filter(|o| !o.is_empty())
            .map(|o| o[..o.len() - 1].to_vec())
            .filter(|p| {
                new.at(p)
                    .is_some_and(|n| n.layout != crate::resolve::Layout::None)
            })
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
            Vec::new(),
            root,
            t,
            Place::default(),
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

/// What a patch changed, by order in the drawing: nodes whose own
/// attributes changed, subtrees that went — removed, moved away, or made
/// again because a node became a leaf of the layout tree or stopped being
/// one — where they were, and subtrees placed where they are now.
#[derive(Default, Debug)]
pub(super) struct Changes {
    pub(super) attrs: Vec<Order>,
    pub(super) added: Vec<Order>,
    pub(super) removed: Vec<Order>,
}

impl Changes {
    /// What `applied` did, found in the drawing `d` showed before and in
    /// `new`: each subtree's root once — none under another — and each in
    /// tree order.
    pub(super) fn of(d: &Drawing, new: &Scene, applied: &crate::wire::Applied) -> Changes {
        let mut removed: BTreeSet<Order> = applied
            .removed
            .iter()
            .filter_map(|id| d.orders.get(id).cloned())
            .collect();
        let mut added: BTreeSet<Order> = applied
            .placed
            .iter()
            .filter_map(|id| new.ids.get(id).cloned())
            .collect();
        let mut attrs = BTreeSet::new();
        let under =
            |set: &BTreeSet<Order>, o: &[OrderKey]| (0..=o.len()).any(|k| set.contains(&o[..k]));
        for (id, old) in &applied.changed {
            let Some(o) = new.ids.get(id) else { continue };
            // A node placed in this patch is made whole.
            if !d.orders.get(id).is_some_and(|was| was == o) || under(&added, o) {
                continue;
            }
            let node = new.at(o).expect("indexed");
            if leafy(old) != leafy(node) {
                removed.insert(o.clone());
                added.insert(o.clone());
                continue;
            }
            attrs.insert(o.clone());
            // Its layout decides part of each child's style.
            if old.layout != node.layout {
                for k in node.children.entries().map(|(k, _)| k) {
                    let mut c = o.clone();
                    c.push(k.clone());
                    attrs.insert(c);
                }
            }
        }
        let tops = |set: &BTreeSet<Order>| -> Vec<Order> {
            set.iter()
                .filter(|o| !(0..o.len()).any(|k| set.contains(&o[..k])))
                .cloned()
                .collect()
        };
        let removed = tops(&removed);
        let added = tops(&added);
        let attrs = attrs
            .into_iter()
            .filter(|o| !under(&added.iter().cloned().collect(), o))
            .collect();
        Changes {
            attrs,
            added,
            removed,
        }
    }

    /// The nodes of the new scene a change is at: every changed and placed
    /// node, and the parent of each removed one.
    pub(super) fn touched(&self) -> impl Iterator<Item = &[OrderKey]> {
        self.attrs
            .iter()
            .chain(&self.added)
            .map(|o| o.as_slice())
            .chain(
                self.removed
                    .iter()
                    .filter(|o| !o.is_empty())
                    .map(|o| &o[..o.len() - 1]),
            )
    }
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
            self.nodes.remove(&o);
            if let Some(rec) = self.recs.remove(&o) {
                if self.orders.get(&rec.node) == Some(&o) {
                    self.orders.remove(&rec.node);
                }
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
