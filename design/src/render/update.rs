//! An update: a drawing brought from one scene to the next by what changed
//! between them, node by node, and the damage it leaves.

use std::collections::{BTreeSet, HashMap};

use kurbo::{Rect as KRect, Vec2};

use crate::display::{Order, Space};
use crate::resolve::{Node, Scene};
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
            images |= self.unplace(d, new, order, &mut damage);
        }
        // What is placed, parents before children and siblings in order, so
        // the siblings before each are in the layout tree already.
        for order in &changes.added {
            self.place(d, new, order);
        }
        for order in &changes.attrs {
            restyle(d, new, order);
        }
        // Every frame with children is a run, which hugs them itself.
        d.laid.settle_top(self);
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
        // The nodes whose attributes that place or cut their children
        // changed; a node's look alone reaches none of them.
        let placing: BTreeSet<Order> = applied
            .changed
            .iter()
            .filter_map(|(id, old)| {
                let o = new.ids.get(id)?;
                let n = new.at(o)?;
                let moves = old.layout != n.layout
                    || old.gap != n.gap
                    || old.padding != n.padding
                    || old.align != n.align
                    || old.justify != n.justify
                    || old.width != n.width
                    || old.height != n.height
                    || old.clip != n.clip
                    || old.radius != n.radius;
                moves.then(|| o.clone())
            })
            .collect();
        let relaid = |o: &Order| placing.contains(o);
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
                    && !d.laid.run_of.contains_key(p)
            })
            .collect();
        let mut walk = Walk {
            scene: new,
            dirty: &dirty,
            relaid: &relaid,
            reflow: Some(&reflow),
            path: Some(&path),
            damage: Some(&mut damage),
            fresh: None,
            images: false,
        };
        let root = d.laid.root;
        self.sync(
            d,
            &mut walk,
            &new.root,
            Vec::new(),
            Owner::Top,
            root,
            t,
            Place::default(),
            (&Space::Root, Vec2::ZERO),
            c,
            None,
            None,
            (false, false),
            None,
        );
        d.list.flush();
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
            // A frame turning to no layout, or from it, is another kind of
            // run — a free one, or one along a line: made again.
            let free = |n: &Node| n.layout == crate::resolve::Layout::None;
            if leafy(old) != leafy(node) || (free(old) != free(node) && !node.children.is_empty()) {
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
        // A frame that starts or stops being a run is laid out another way:
        // made again whole. Only what can turn it is looked at — its own
        // `layout` or `justify`, a child turning `"fill"` along it or no
        // longer, a child coming or going — never its children one by one,
        // unless one of those happened.
        let mut maybe: BTreeSet<Order> = BTreeSet::new();
        let along = |n: &Node, c: &Node| match n.layout {
            crate::resolve::Layout::Row => c.width,
            crate::resolve::Layout::Column => c.height,
            crate::resolve::Layout::None => crate::resolve::Size::Hug,
        };
        let fill = crate::resolve::Size::Fill;
        for (id, old) in &applied.changed {
            let Some(o) = new.ids.get(id) else { continue };
            let Some(node) = new.at(o) else { continue };
            if old.layout != node.layout || old.justify != node.justify {
                maybe.insert(o.clone());
            }
            // A child turning `"fill"` along its run, or no longer, is laid
            // out another way in it: placed again.
            if !o.is_empty()
                && let Some(p) = new.at(&o[..o.len() - 1])
                && ((along(p, old) == fill) != (along(p, node) == fill))
                && !under(&added, o)
            {
                removed.insert(o.clone());
                added.insert(o.clone());
            }
        }
        for o in added.iter().chain(&removed) {
            if !o.is_empty() {
                maybe.insert(o[..o.len() - 1].to_vec());
            }
        }
        for p in maybe {
            let Some(n) = new.at(&p) else { continue };
            if d.orders.get(&n.id) != Some(&p) || under(&added, &p) {
                continue;
            }
            let was = d.laid.run_of.contains_key(&p);
            // A run stays one while every child it gains takes its own
            // length and it keeps some: no scan of its children.
            let now = if was
                && !n.children.is_empty()
                && n.justify != crate::resolve::Align::SpaceBetween
                && n.layout != crate::resolve::Layout::None
                && added
                    .iter()
                    .filter(|o| o.len() == p.len() + 1 && o.starts_with(&p))
                    .all(|o| new.at(o).is_none_or(|c| along(n, c) != fill))
                && !applied.changed.iter().any(|(id, _)| {
                    new.ids.get(id).is_some_and(|o| {
                        o.len() == p.len() + 1
                            && o.starts_with(&p)
                            && new.at(o).is_some_and(|c| along(n, c) == fill)
                    })
                }) {
                true
            } else {
                is_run(n)
            };
            if was != now {
                removed.insert(p.clone());
                added.insert(p);
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
    /// Takes out everything at and under `order` — chunks, records, frames,
    /// runs and the property-tree nodes they made — adding where the chunks
    /// were to `damage`. True when any of them painted an image.
    pub(super) fn forget(&mut self, order: &Order, damage: &mut Damage) -> bool {
        let under: Vec<Order> = self
            .recs
            .range(order.clone()..)
            .take_while(|(o, _)| o.starts_with(order))
            .map(|(o, _)| o.clone())
            .collect();
        let mut images = false;
        for o in under.iter().rev() {
            let at = self.list.chunk(o).map(|c| self.list.space_at(&c.space));
            if let Some(c) = self.list.take(o) {
                let at = at.unwrap_or(Vec2::ZERO);
                damage.rects.extend(c.bounds.map(|b| b + at));
                self.list.free(&c.made);
            }
            if let Some(rid) = self.laid.run_of.get(o)
                && let Some(drid) = self.shown.remove(rid)
            {
                self.list.remove_run(drid);
                self.shifts.remove(&drid);
                self.contexts.remove(&drid);
            }
            if let Some(rec) = self.recs.remove(o) {
                if self.orders.get(&rec.node) == Some(o) {
                    self.orders.remove(&rec.node);
                }
                images |= !rec.painted.is_empty();
                self.painters.remove(o);
                self.list.trees.transforms.remove(rec.transform);
                if let Some(e) = rec.effect {
                    self.list.trees.effects.remove(e);
                }
                if let Some(c) = rec.clip {
                    self.list.trees.clips.remove(c);
                }
            }
            self.frames.remove(o);
        }
        images
    }

    /// The key of the run's child that `order` is inside, when the node is in
    /// a run's tree.
    pub(super) fn inside(&self, owner: Owner, order: &Order) -> Option<OrderKey> {
        match owner {
            Owner::Top => None,
            Owner::Run(p) => {
                let at = self.laid.runs.get(p).order.len();
                order.get(at).cloned()
            }
        }
    }
}

impl Renderer {
    /// Takes the subtree at `order` out: from the list, and from the layout
    /// tree it is in — a run's child out of its run.
    fn unplace(
        &mut self,
        d: &mut Drawing,
        new: &Scene,
        order: &Order,
        damage: &mut Damage,
    ) -> bool {
        let parent = (!order.is_empty()).then(|| order[..order.len() - 1].to_vec());
        let run = parent.as_ref().and_then(|p| d.laid.run_of.get(p).copied());
        let key = order.last().cloned();
        if !order.is_empty() {
            d.laid.sens_drop(&new.root, order);
        }
        // Where a run's child started: what follows it moves from there.
        if let (Some(rid), Some(k)) = (run, &key)
            && let Some(&drid) = d.shown.get(&rid)
            && let Some(at) = d.list.run(drid).child_at(k)
        {
            let r = d.list.run(drid);
            let abs = d.list.space_at(&r.space) + at;
            let along = if r.row { abs.x } else { abs.y };
            let s = d.shifts.entry(drid).or_insert(along);
            *s = s.min(along);
        }
        let images = d.forget(order, damage);
        if let (Some(rid), Some(k)) = (run, &key)
            && let Some(&drid) = d.shown.get(&rid)
        {
            d.list.place_child(drid, k.clone(), None);
        }
        if !d.laid.nodes.contains_key(order) {
            return images;
        }
        // The layout's records of everything under it, its runs freed.
        let nodes: Vec<Order> = d
            .laid
            .nodes
            .range(order.clone()..)
            .take_while(|(o, _)| o.starts_with(order))
            .map(|(o, _)| o.clone())
            .collect();
        let mut freed = Vec::new();
        for o in &nodes {
            d.laid.nodes.remove(o);
            if let Some(rid) = d.laid.run_of.remove(o) {
                freed.push(rid);
            }
        }
        if order.is_empty() {
            for rid in freed {
                d.laid.runs.free(rid);
            }
            return images;
        }
        match (run, key) {
            (Some(rid), Some(k)) => {
                let r = d.laid.runs.get_mut(rid);
                if let Some(c) = r.remove(&k) {
                    remove_subtree(&mut r.tree, c.node);
                }
                let laid = &mut d.laid;
                laid.runs.touch(&mut laid.top, rid, k);
            }
            // A frame that loses its last child stops being a run and is
            // made again whole: every parent here is one.
            _ => unreachable!("a child taken from a frame that isn't a run"),
        }
        for rid in freed {
            d.laid.runs.free(rid);
        }
        images
    }

    /// Puts the subtree at `order` in the layout: into its parent's run —
    /// every frame with children is one — or as a new root.
    fn place(&mut self, d: &mut Drawing, new: &Scene, order: &Order) {
        let node = find_node(&new.root, order);
        if order.is_empty() {
            d.laid = Laid::build(self, new);
            return;
        }
        let parent_order = order[..order.len() - 1].to_vec();
        let parent = find_node(&new.root, &parent_order);
        let key = order.last().expect("a key").clone();
        if d.laid.sens_build(node, order.clone()) {
            d.laid.sens_settle(&new.root, &parent_order, 1);
        }
        if let Some(&rid) = d.laid.run_of.get(&parent_order) {
            let mut run = d.laid.runs.take(rid);
            d.laid.add_child(
                &mut run,
                rid,
                &key,
                node,
                parent,
                &new.images,
                &parent_order,
            );
            d.laid.runs.put(rid, run);
            let laid = &mut d.laid;
            laid.runs.touch(&mut laid.top, rid, key);
            return;
        }
        // A frame that gains its first child becomes a run, and is made
        // again whole: every parent here is one.
        unreachable!("a child placed under a frame that isn't a run");
    }
}

/// A node whose own attributes changed, restyled in its layout tree — a run
/// told how it now sets its children.
fn restyle(d: &mut Drawing, new: &Scene, order: &Order) {
    let node = find_node(&new.root, order);
    let parent = (!order.is_empty()).then(|| find_node(&new.root, &order[..order.len() - 1]));
    let Some(&(owner, id)) = d.laid.nodes.get(order) else {
        return;
    };
    let style = style_of(node, parent);
    d.laid.sens_settle(&new.root, order, 0);
    if let Some(&rid) = d.laid.run_of.get(order) {
        // Its children are laid out again only along a new direction; its
        // leaf, restyled below, is measured again.
        let run = d.laid.runs.get_mut(rid);
        run.set(node);
        run.hug = hugs_by_extent(node, parent).unwrap_or((false, false));
    }
    let tree = d.laid.tree_mut(owner);
    tree.set_style(id, style).expect("style");
    if let Some(leaf) = leaf_of(node, &new.images) {
        tree.set_node_context(id, Some(Some(leaf))).expect("leaf");
    }
    if let Some(p) = parent
        && let Some(&rid) = d.laid.run_of.get(&order[..order.len() - 1])
    {
        let k = order.last().expect("a key").clone();
        let row = d.laid.runs.get(rid).row;
        let across = if row { node.height } else { node.width };
        let sens = d.laid.sensitive(order);
        d.laid
            .runs
            .get_mut(rid)
            .classify(&k, across == crate::resolve::Size::Fill, sens);
        let _ = p;
        let laid = &mut d.laid;
        laid.runs.touch(&mut laid.top, rid, k);
        return;
    }
    if let Some(k) = d.inside(owner, order)
        && let Owner::Run(p) = owner
    {
        let laid = &mut d.laid;
        laid.runs.touch(&mut laid.top, p, k);
    }
}

/// What an edit costs, by the kind of frame it is in.
#[cfg(test)]
mod costs {
    mod tests;
}
#[cfg(test)]
mod tests;
