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
        let mut hugs: Vec<(Order, NodeId, bool, bool)> = Vec::new();
        // What is gone: subtrees removed, moved away or made again — their
        // layout nodes, chunks, records and property-tree nodes.
        for order in &changes.removed {
            images |= self.unplace(d, new, order, &mut damage);
        }
        // What is placed, parents before children and siblings in order, so
        // the siblings before each are in the layout tree already.
        for order in &changes.added {
            self.place(d, new, order, &mut hugs);
        }
        for order in &changes.attrs {
            restyle(d, new, order);
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
                if let Some((w, h)) = hugs_of(node, parent)
                    && let Some(&(Owner::Top, id)) = d.laid.nodes.get(at)
                {
                    hugs.push((at.to_vec(), id, w, h));
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
            let mut style = d.laid.top.style(id).expect("style").clone();
            if w {
                style.size.width = Dimension::auto();
            }
            if h {
                style.size.height = Dimension::auto();
            }
            d.laid.top.set_style(id, style).expect("style");
        }
        d.laid.settle_top(self, &hugs);
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
                    && !d.laid.run_of.contains_key(p)
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
            if !o.is_empty()
                && let Some(p) = new.at(&o[..o.len() - 1])
                && ((along(p, old) == fill) != (along(p, node) == fill))
            {
                maybe.insert(o[..o.len() - 1].to_vec());
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
        let Some(&(_, gone)) = d.laid.nodes.get(order) else {
            return images;
        };
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
        let parent = parent.expect("not the root");
        match (run, key) {
            (Some(rid), Some(k)) => {
                let r = d.laid.runs.get_mut(rid);
                if let Some(c) = r.remove(&k) {
                    remove_subtree(&mut r.tree, c.node);
                }
                let laid = &mut d.laid;
                laid.runs.touch(&mut laid.top, rid, k);
            }
            _ => {
                let (p_owner, pid) = d.laid.nodes[&parent];
                let tree = d.laid.tree_mut(p_owner);
                tree.remove_child(pid, gone).expect("child");
                remove_subtree(tree, gone);
                if tree.child_count(pid) == 0 {
                    // A frame left with no children is laid out as a leaf,
                    // which never clears the list of absolutely placed
                    // children taffy kept for it — the ones just removed:
                    // it is made afresh.
                    let node = find_node(&new.root, &parent);
                    let up = (!parent.is_empty())
                        .then(|| find_node(&new.root, &parent[..parent.len() - 1]));
                    let fresh = tree.new_leaf(style_of(node, up)).expect("node");
                    if parent.is_empty() {
                        d.laid.root = fresh;
                    } else {
                        let grand = &parent[..parent.len() - 1];
                        match d.laid.run_of.get(grand).copied() {
                            Some(grid) => {
                                // The frame is a run's child: its one-item
                                // container holds it.
                                let k = parent.last().expect("a key").clone();
                                let r = d.laid.runs.get_mut(grid);
                                let wrap = r.children[&k].wrap;
                                r.tree
                                    .replace_child_at_index(wrap, 0, fresh)
                                    .expect("child");
                                r.children.get_mut(&k).expect("a child").node = fresh;
                            }
                            None => {
                                let gid = d.laid.nodes[grand].1;
                                let tree = d.laid.tree_mut(p_owner);
                                let i = tree
                                    .children(gid)
                                    .expect("children")
                                    .iter()
                                    .position(|&c| c == pid)
                                    .expect("a child of its parent");
                                tree.replace_child_at_index(gid, i, fresh).expect("child");
                            }
                        }
                    }
                    d.laid.nodes.insert(parent.clone(), (p_owner, fresh));
                    let _ = d.laid.tree_mut(p_owner).remove(pid);
                }
                if let Some(k) = d.inside(p_owner, &parent)
                    && let Owner::Run(p) = p_owner
                {
                    self.rehug(d, new, p, &k);
                    let laid = &mut d.laid;
                    laid.runs.touch(&mut laid.top, p, k);
                }
            }
        }
        for rid in freed {
            d.laid.runs.free(rid);
        }
        images
    }

    /// Puts the subtree at `order` in the layout: a run's child into its
    /// run, anything else into its parent's tree; the frames in the drawing's
    /// own tree that hug, to `hugs`.
    fn place(
        &mut self,
        d: &mut Drawing,
        new: &Scene,
        order: &Order,
        hugs: &mut Vec<(Order, NodeId, bool, bool)>,
    ) {
        let node = find_node(&new.root, order);
        if order.is_empty() {
            let mut laid = Laid::build(self, new);
            let fresh = std::mem::take(&mut laid.hugs);
            hugs.extend(fresh.into_iter().map(|(id, w, h)| (Vec::new(), id, w, h)));
            d.laid = laid;
            return;
        }
        let parent_order = order[..order.len() - 1].to_vec();
        let parent = find_node(&new.root, &parent_order);
        let key = order.last().expect("a key").clone();
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
        let (owner, pid) = d.laid.nodes[&parent_order];
        let inside = d.inside(owner, &parent_order);
        let mut tree = std::mem::replace(d.laid.tree_mut(owner), TaffyTree::new());
        let mut hug = Vec::new();
        let id = d.laid.build_node(
            &mut tree,
            owner,
            inside.clone(),
            node,
            Some(parent),
            &mut hug,
            &new.images,
            order.clone(),
        );
        let place = parent.children.rank(&key);
        tree.insert_child_at_index(pid, place, id).expect("child");
        *d.laid.tree_mut(owner) = tree;
        match (owner, inside) {
            (Owner::Run(p), Some(k)) => {
                self.rehug(d, new, p, &k);
                let laid = &mut d.laid;
                laid.runs.touch(&mut laid.top, p, k);
            }
            _ => hugs.extend(hug.into_iter().map(|(id, w, h)| (order.clone(), id, w, h))),
        }
    }

    /// The frames that hug in run `rid`'s child `key`, found again.
    fn rehug(&mut self, d: &mut Drawing, new: &Scene, rid: u32, key: &OrderKey) {
        let mut o = d.laid.runs.get(rid).order.clone();
        o.push(key.clone());
        let mut out = Vec::new();
        fn walk(
            d: &Drawing,
            node: &Node,
            parent: Option<&Node>,
            o: &mut Order,
            rid: u32,
            out: &mut Vec<(NodeId, bool, bool)>,
        ) {
            let Some(&(owner, id)) = d.laid.nodes.get(o) else {
                return;
            };
            if owner != Owner::Run(rid) {
                return;
            }
            if let Some((w, h)) = hugs_of(node, parent) {
                out.push((id, w, h));
            }
            if d.laid.run_of.contains_key(o) {
                return;
            }
            for (k, c) in node.children.entries() {
                o.push(k.clone());
                walk(d, c, Some(node), o, rid, out);
                o.pop();
            }
        }
        let item = find_node(&new.root, &o);
        let parent = find_node(&new.root, &o[..o.len() - 1]);
        walk(d, item, Some(parent), &mut o, rid, &mut out);
        // Innermost first, as a fresh build lists them.
        out.reverse();
        if let Some(c) = d.laid.runs.get_mut(rid).children.get_mut(key) {
            c.hugs = out;
        }
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
    if let Some(&rid) = d.laid.run_of.get(order) {
        d.laid.runs.get_mut(rid).set(node);
    }
    let tree = d.laid.tree_mut(owner);
    tree.set_style(id, style).expect("style");
    if let Some(leaf) = leaf_of(node, &new.images) {
        tree.set_node_context(id, Some(Some(leaf))).expect("leaf");
    }
    if let Some(&rid) = d.laid.run_of.get(order) {
        // Its children are laid out again under how it now sets them.
        let keys: Vec<OrderKey> = d.laid.runs.get(rid).children.keys().cloned().collect();
        let laid = &mut d.laid;
        for k in keys {
            laid.runs.touch(&mut laid.top, rid, k);
        }
    }
    if let Some(p) = parent
        && let Some(&rid) = d.laid.run_of.get(&order[..order.len() - 1])
    {
        let k = order.last().expect("a key").clone();
        let row = d.laid.runs.get(rid).row;
        let across = if row { node.height } else { node.width };
        if let Some(c) = d.laid.runs.get_mut(rid).children.get_mut(&k) {
            c.stretch = across == crate::resolve::Size::Fill;
        }
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

#[cfg(test)]
mod tests;
