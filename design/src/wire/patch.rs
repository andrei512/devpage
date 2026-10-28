//! Patches: the operations that turn one scene into the next, each naming
//! its nodes by id — a node's own attributes changed, a subtree inserted
//! after a sibling, a subtree removed, a subtree moved — and applying them to
//! a scene, all or nothing, at the cost of the operations alone.

use std::collections::{HashMap, HashSet};

use super::{
    Uses, decode, encode, encode_own, file, files, font_file, font_files, node_id, unnode_id,
};
use crate::proto::{self, op::Op as Kind};
use crate::resolve::{Node, Scene};
use crate::tree::{NodeId, Order, OrderKey};

/// The patch that turns `old`, as last sent, into `new`, by their nodes'
/// ids: an insert before the first of 100,000 siblings is one insert. And the
/// image and font files `old` didn't have.
///
/// It compares the two scenes whole; the store finds most patches without
/// one, from what a save reached.
pub fn patch(old: &Scene, new: &Scene) -> proto::Patch {
    proto::Patch {
        ops: ops(old, new),
        images: new
            .images
            .values()
            .filter(|f| !old.images.contains_key(&f.id))
            .map(|f| file(f))
            .collect(),
        fonts: new
            .fonts
            .values()
            .filter(|f| !old.fonts.contains_key(&f.id))
            .map(|f| font_file(f))
            .collect(),
        themed: new.themed,
    }
}

fn op(kind: Kind) -> proto::Op {
    proto::Op { op: Some(kind) }
}

/// The operations from `old` to `new`. Each node of `new` that `old` has is
/// kept — moved if it has another parent or falls out of the longest run of
/// siblings still in their order, and changed if its own attributes did;
/// each one `old` hasn't is inserted, whole where nothing under it is kept,
/// alone where something is, which is then moved into it. Last, what `old`
/// has and `new` doesn't goes: by then nothing kept is under it.
fn ops(old: &Scene, new: &Scene) -> Vec<proto::Op> {
    if old.root.id != new.root.id {
        return vec![op(Kind::Insert(proto::Insert {
            parent: None,
            after: None,
            node: Some(encode(&new.root)),
        }))];
    }
    let mut out = Vec::new();
    // Under each node of `new`, whether something `old` has is.
    fn kept_below(n: &Node, old: &Scene, out: &mut HashSet<NodeId>) -> bool {
        let mut any = false;
        for c in &n.children {
            any |= kept_below(c, old, out) | old.ids.contains(&c.id);
        }
        if any {
            out.insert(n.id.clone());
        }
        any
    }
    let mut holds = HashSet::new();
    kept_below(&new.root, old, &mut holds);
    if !old.root.same_own(&new.root) {
        out.push(op(Kind::Change(encode_own(&new.root))));
    }
    let mut todo = vec![&new.root];
    while let Some(p) = todo.pop() {
        // The children `old` had under this same parent, by their places
        // there: those in the longest run still in order stay.
        let old_parent = old.ids.get(&p.id).and_then(|o| old.at(o));
        let rank: HashMap<&NodeId, usize> = old_parent
            .map(|op| {
                op.children
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (&c.id, i))
                    .collect()
            })
            .unwrap_or_default();
        let ranks: Vec<Option<usize>> = p
            .children
            .iter()
            .map(|c| rank.get(&c.id).copied())
            .collect();
        let stay = longest_run(&ranks);
        let mut prev: Option<&NodeId> = None;
        for (i, c) in p.children.iter().enumerate() {
            let at = |id: Option<&NodeId>| id.map(node_id);
            match old.node(&c.id) {
                Some(was) => {
                    if !stay[i] {
                        out.push(op(Kind::Move(proto::Move {
                            id: Some(node_id(&c.id)),
                            parent: Some(node_id(&p.id)),
                            after: at(prev),
                        })));
                    }
                    if !was.same_own(c) {
                        out.push(op(Kind::Change(encode_own(c))));
                    }
                    todo.push(c);
                }
                None => {
                    let node = if holds.contains(&c.id) {
                        todo.push(c);
                        encode_own(c)
                    } else {
                        encode(c)
                    };
                    out.push(op(Kind::Insert(proto::Insert {
                        parent: Some(node_id(&p.id)),
                        after: at(prev),
                        node: Some(node),
                    })));
                }
            }
            prev = Some(&c.id);
        }
    }
    fn gone(n: &Node, new: &Scene, out: &mut Vec<proto::Op>) {
        for c in &n.children {
            if new.ids.contains(&c.id) {
                gone(c, new, out);
            } else {
                out.push(op(Kind::Remove(node_id(&c.id))));
            }
        }
    }
    gone(&old.root, new, &mut out);
    out
}

/// Which of a list stay where they are: the longest increasing run of their
/// places before, `None` for those that weren't there.
fn longest_run(ranks: &[Option<usize>]) -> Vec<bool> {
    // Patience sorting: the smallest last place of a run of each length,
    // and each element's predecessor in its run.
    let mut tails: Vec<usize> = Vec::new();
    let mut before: Vec<Option<usize>> = vec![None; ranks.len()];
    for (i, r) in ranks.iter().enumerate() {
        let Some(r) = r else { continue };
        let at = tails.partition_point(|&t| ranks[t].expect("ranked") < *r);
        before[i] = at.checked_sub(1).map(|a| tails[a]);
        if at == tails.len() {
            tails.push(i);
        } else {
            tails[at] = i;
        }
    }
    let mut stay = vec![false; ranks.len()];
    let mut at = tails.last().copied();
    while let Some(i) = at {
        stay[i] = true;
        at = before[i];
    }
    stay
}

/// What a patch did, by id, for a drawing kept beside the scene to follow:
/// the subtrees that left their place — removed, or moved away — the
/// subtrees placed — inserted, or moved here — and the nodes that kept their
/// place and took new own attributes, with the ones they had.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Applied {
    pub removed: Vec<NodeId>,
    pub placed: Vec<NodeId>,
    pub changed: Vec<(NodeId, Node)>,
}

/// One operation done, and how to undo it.
enum Undo {
    /// Put back these own attributes at this order.
    Own(Order, Node),
    /// Take out the subtree at this order.
    Take(Order),
    /// Put this subtree back under the parent at this order, under this key.
    Put(Order, OrderKey, Node),
    /// Move the subtree now at this order back under this parent and key.
    Back(Order, Order, OrderKey),
    /// Put this root back.
    Root(Node),
}

/// Applies a patch to `scene`, whose files `uses` counts, or says why it
/// can't — leaving both as they were. The work is the patch's: each
/// operation finds its nodes by id, and the files no node uses any more are
/// dropped by count, never by a walk of the scene.
pub fn apply(scene: &mut Scene, uses: &mut Uses, patch: &proto::Patch) -> Result<Applied, String> {
    let images = files(&patch.images, &scene.images)?;
    let fonts = font_files(&patch.fonts, &scene.fonts)?;
    apply_ops(scene, uses, &patch.ops, images, fonts, patch.themed)
}

/// Brings `scene`, whose files `uses` counts, to `to`'s content by the
/// patch between them — its nodes keeping their places, so what is kept
/// beside it can follow — with `to`'s image and font files as they are,
/// never through the wire's bytes.
pub fn bring(scene: &mut Scene, uses: &mut Uses, to: &Scene) -> Result<Applied, String> {
    let ops = ops(scene, to);
    let images = to.images.clone();
    let fonts = to.fonts.clone();
    apply_ops(scene, uses, &ops, images, fonts, to.themed)
}

fn apply_ops(
    scene: &mut Scene,
    uses: &mut Uses,
    ops: &[proto::Op],
    images: crate::resolve::ImageFiles,
    fonts: crate::fonts::FontFiles,
    themed: bool,
) -> Result<Applied, String> {
    let mut undo = Vec::new();
    let mut applied = Applied::default();
    let done =
        run(scene, uses, ops, &mut undo, &mut applied).and_then(|()| {
            // Every file a node uses is held or arrives with the patch.
            if let Some(missing) = uses.images.iter().find(|(id, c)| {
                **c > 0 && !scene.images.contains_key(id) && !images.contains_key(id)
            }) {
                return Err(format!(
                    "the stream never sent the image file {:?}",
                    missing.0
                ));
            }
            if let Some(missing) = uses
                .fonts
                .iter()
                .find(|(id, c)| **c > 0 && !scene.fonts.contains_key(id) && !fonts.contains_key(id))
            {
                return Err(format!(
                    "the stream never sent the font file {:?}",
                    missing.0
                ));
            }
            Ok(())
        });
    if let Err(e) = done {
        for u in undo.into_iter().rev() {
            revert(scene, uses, u);
        }
        uses.settle();
        return Err(e);
    }
    scene.images.extend(images);
    scene.fonts.extend(fonts);
    uses.settle();
    scene.images.retain(|id, _| uses.images.contains_key(id));
    scene.fonts.retain(|id, _| uses.fonts.contains_key(id));
    scene.themed = themed;
    Ok(applied)
}

fn run(
    scene: &mut Scene,
    uses: &mut Uses,
    ops: &[proto::Op],
    undo: &mut Vec<Undo>,
    applied: &mut Applied,
) -> Result<(), String> {
    for o in ops {
        match o.op.as_ref().ok_or("an empty operation")? {
            Kind::Change(n) => {
                let new = decode(n)?;
                let order = order_of(scene, &new.id)?;
                let place = scene.at_mut(&order).expect("indexed");
                let mut own = std::mem::replace(place, new);
                std::mem::swap(&mut place.children, &mut own.children);
                uses.take_own(&own);
                uses.add_own(place);
                applied.changed.push((own.id.clone(), own.clone()));
                undo.push(Undo::Own(order, own));
            }
            Kind::Insert(i) => {
                let node = decode(i.node.as_ref().ok_or("an insert with no node")?)?;
                let Some(parent) = i.parent.as_ref() else {
                    // A new root: the scene, replaced.
                    if crate::tree::Ids::of(&node).len() != count(&node) {
                        return Err(format!("{} holds a node twice", node.id));
                    }
                    let old = std::mem::replace(&mut scene.root, node);
                    uses.take(&old);
                    uses.add(&scene.root);
                    scene.ids = crate::tree::Ids::of(&scene.root);
                    applied.removed.push(old.id.clone());
                    applied.placed.push(scene.root.id.clone());
                    undo.push(Undo::Root(old));
                    continue;
                };
                let parent = order_of(scene, &unnode_id(Some(parent))?)?;
                let after = after_key(scene, &parent, i.after.as_ref())?;
                check_unique(scene, &node, None)?;
                uses.add(&node);
                let id = node.id.clone();
                let order = place(scene, &parent, after.as_ref(), node);
                applied.placed.push(id);
                undo.push(Undo::Take(order));
            }
            Kind::Remove(id) => {
                let id = unnode_id(Some(id))?;
                let order = order_of(scene, &id)?;
                if order.is_empty() {
                    return Err("the root can't be removed".into());
                }
                let (parent, key) = (
                    order[..order.len() - 1].to_vec(),
                    order[order.len() - 1].clone(),
                );
                let node = take(scene, &order);
                uses.take(&node);
                applied.removed.push(id);
                undo.push(Undo::Put(parent, key, node));
            }
            Kind::Move(m) => {
                let id = unnode_id(m.id.as_ref())?;
                let from = order_of(scene, &id)?;
                if from.is_empty() {
                    return Err("the root can't be moved".into());
                }
                let parent = order_of(scene, &unnode_id(m.parent.as_ref())?)?;
                if parent.starts_with(&from) {
                    return Err(format!("{id} can't move under itself"));
                }
                let key = from[from.len() - 1].clone();
                let old_parent = from[..from.len() - 1].to_vec();
                let after = after_key(scene, &parent, m.after.as_ref())?;
                if after.as_ref() == Some(&key) && parent == old_parent {
                    return Err(format!("{id} can't move after itself"));
                }
                let node = take(scene, &from);
                let to = place(scene, &parent, after.as_ref(), node);
                applied.removed.push(id.clone());
                applied.placed.push(id);
                undo.push(Undo::Back(to, old_parent, key));
            }
        }
    }
    Ok(())
}

fn count(n: &Node) -> usize {
    1 + n.children.iter().map(count).sum::<usize>()
}

fn order_of(scene: &Scene, id: &NodeId) -> Result<Order, String> {
    scene
        .ids
        .get(id)
        .cloned()
        .ok_or_else(|| format!("no node {id}"))
}

/// The key, under the parent at `parent`, of its child `after`.
fn after_key(
    scene: &Scene,
    parent: &Order,
    after: Option<&proto::NodeId>,
) -> Result<Option<OrderKey>, String> {
    let Some(after) = after else { return Ok(None) };
    let id = unnode_id(Some(after))?;
    let o = order_of(scene, &id)?;
    if o.len() != parent.len() + 1 || !o.starts_with(parent) {
        return Err(format!("{id} isn't a child of the node it is placed under"));
    }
    Ok(o.last().cloned())
}

/// Says if `node`'s subtree names a node the scene has, but for `but`.
fn check_unique(scene: &Scene, node: &Node, but: Option<&NodeId>) -> Result<(), String> {
    if Some(&node.id) != but && scene.ids.contains(&node.id) {
        return Err(format!("{} is in the scene already", node.id));
    }
    let mut seen = HashSet::new();
    fn walk<'a>(n: &'a Node, scene: &Scene, seen: &mut HashSet<&'a NodeId>) -> Result<(), String> {
        if !seen.insert(&n.id) {
            return Err(format!("{} is in the subtree twice", n.id));
        }
        for c in &n.children {
            if scene.ids.contains(&c.id) {
                return Err(format!("{} is in the scene already", c.id));
            }
            walk(c, scene, seen)?;
        }
        Ok(())
    }
    walk(node, scene, &mut seen)
}

/// Places `node` under the parent at `parent`, after its child `after` or
/// first, indexing its subtree; returns where it is.
fn place(scene: &mut Scene, parent: &Order, after: Option<&OrderKey>, node: Node) -> Order {
    let p = scene.at_mut(parent).expect("indexed");
    let key = p.children.insert_after(after, node);
    let mut order = parent.clone();
    order.push(key.clone());
    index(scene, &order);
    order
}

/// Indexes the subtree at `order`.
fn index(scene: &mut Scene, order: &Order) {
    let mut at = &scene.root;
    for k in order {
        at = at.children.get(k).expect("placed");
    }
    scene.ids.add(at, order.clone());
}

/// Takes the subtree at `order` out of the scene and its index.
fn take(scene: &mut Scene, order: &Order) -> Node {
    let (parent, key) = (&order[..order.len() - 1], &order[order.len() - 1]);
    let node = scene
        .at_mut(parent)
        .expect("indexed")
        .children
        .remove(key)
        .expect("indexed");
    scene.ids.remove(&node);
    node
}

fn revert(scene: &mut Scene, uses: &mut Uses, u: Undo) {
    match u {
        Undo::Own(order, mut own) => {
            let place = scene.at_mut(&order).expect("done");
            std::mem::swap(&mut place.children, &mut own.children);
            let new = std::mem::replace(place, own);
            uses.take_own(&new);
            uses.add_own(scene.at(&order).expect("done"));
        }
        Undo::Take(order) => {
            let node = take(scene, &order);
            uses.take(&node);
        }
        Undo::Put(parent, key, node) => {
            uses.add(&node);
            let p = scene.at_mut(&parent).expect("done");
            p.children.put(key.clone(), node);
            let mut order = parent;
            order.push(key);
            index(scene, &order);
        }
        Undo::Back(now, parent, key) => {
            let node = take(scene, &now);
            let p = scene.at_mut(&parent).expect("done");
            p.children.put(key.clone(), node);
            let mut order = parent;
            order.push(key);
            index(scene, &order);
        }
        Undo::Root(old) => {
            let new = std::mem::replace(&mut scene.root, old);
            uses.take(&new);
            uses.add(&scene.root);
            scene.ids = crate::tree::Ids::of(&scene.root);
        }
    }
}

#[cfg(test)]
mod tests;
