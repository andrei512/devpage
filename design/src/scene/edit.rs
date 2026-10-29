//! Edits: how one scene becomes the next. Each names its nodes by id — a
//! node's own attributes changed, a subtree inserted after a sibling, a
//! subtree removed, a subtree moved — and applying them to a scene is all or
//! nothing, at the cost of the edits alone.

use std::collections::{HashMap, HashSet};

use super::{Ids, ImageFiles, ImageId, Node, Scene};
use crate::fonts::{FontFiles, FontId};
use crate::tree::{NodeId, Order, OrderKey};

/// One step from a scene to the next, naming its nodes by id.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    /// The node of this id takes these own attributes; its children stay.
    Change(Node),
    /// This subtree goes under the node `parent`, after its child `after`
    /// or first — or, with no parent, replaces the scene.
    Insert {
        parent: Option<NodeId>,
        after: Option<NodeId>,
        node: Node,
    },
    /// The subtree of this id goes.
    Remove(NodeId),
    /// The subtree of this id moves under `parent`, after `after` or first.
    Move {
        id: NodeId,
        parent: NodeId,
        after: Option<NodeId>,
    },
}

/// Edits with the image and font files they bring: everything that takes a
/// scene to the next. Of the files, only those the scene before didn't hold
/// — each travels once.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Edits {
    pub list: Vec<Edit>,
    pub images: ImageFiles,
    pub fonts: FontFiles,
    /// Whether the scene they make is themed.
    pub themed: bool,
}

impl Edits {
    /// The edits that turn `old` into `new`, by their nodes' ids: an insert
    /// before the first of 100,000 siblings is one insert. It compares the
    /// two scenes whole; a design resolved again finds most edits without
    /// one, from what a save reached.
    pub fn between(old: &Scene, new: &Scene) -> Edits {
        Edits::onto(old, diff(old, new), &new.images, &new.fonts, new.themed)
    }

    /// `list` to apply to `scene`, with of `images` and `fonts` — the files
    /// the scene they make may use — those `scene` doesn't hold.
    pub fn onto(
        scene: &Scene,
        list: Vec<Edit>,
        images: &ImageFiles,
        fonts: &FontFiles,
        themed: bool,
    ) -> Edits {
        Edits {
            list,
            images: images
                .iter()
                .filter(|(id, _)| !scene.images.contains_key(id))
                .map(|(id, f)| (*id, f.clone()))
                .collect(),
            fonts: fonts
                .iter()
                .filter(|(id, _)| !scene.fonts.contains_key(id))
                .map(|(id, f)| (*id, f.clone()))
                .collect(),
            themed,
        }
    }

    /// `scene` whole, as edits from nothing: its root, and every file.
    pub fn whole(scene: &Scene) -> Edits {
        Edits {
            list: vec![Edit::Insert {
                parent: None,
                after: None,
                node: scene.root.clone(),
            }],
            images: scene.images.clone(),
            fonts: scene.fonts.clone(),
            themed: scene.themed,
        }
    }
}

/// The edits from `old` to `new`. Each node of `new` that `old` has is
/// kept — moved if it has another parent or falls out of the longest run of
/// siblings still in their order, and changed if its own attributes did;
/// each one `old` hasn't is inserted, whole where nothing under it is kept,
/// alone where something is, which is then moved into it. Last, what `old`
/// has and `new` doesn't goes: by then nothing kept is under it.
fn diff(old: &Scene, new: &Scene) -> Vec<Edit> {
    if old.root.id != new.root.id {
        return vec![Edit::Insert {
            parent: None,
            after: None,
            node: new.root.clone(),
        }];
    }
    subtree(old, &[], &new.root)
}

/// The edits that turn the subtree of `old` at `at` — whose root has the id
/// `new`'s root has — into `new`: what a part of a scene resolved again is
/// brought across by, costing that part alone.
pub fn subtree(old: &Scene, at: &[OrderKey], new: &Node) -> Vec<Edit> {
    let within = |id: &NodeId| old.ids.get(id).is_some_and(|o| o.starts_with(at));
    let mut out = Vec::new();
    // Under each node of `new`, whether something `old` has is.
    fn kept_below(n: &Node, within: &dyn Fn(&NodeId) -> bool, out: &mut HashSet<NodeId>) -> bool {
        let mut any = false;
        for c in &n.children {
            any |= kept_below(c, within, out) | within(&c.id);
        }
        if any {
            out.insert(n.id.clone());
        }
        any
    }
    let mut holds = HashSet::new();
    kept_below(new, &within, &mut holds);
    let Some(was) = old.at(at) else {
        return out;
    };
    if !was.same_own(new) {
        out.push(Edit::Change(new.own()));
    }
    let mut todo = vec![new];
    while let Some(p) = todo.pop() {
        // The children `old` had under this same parent, by their places
        // there: those in the longest run still in order stay.
        let old_parent = old
            .ids
            .get(&p.id)
            .filter(|o| o.starts_with(at))
            .and_then(|o| old.at(o));
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
            match old.node(&c.id).filter(|_| within(&c.id)) {
                Some(was) => {
                    if !stay[i] {
                        out.push(Edit::Move {
                            id: c.id.clone(),
                            parent: p.id.clone(),
                            after: prev.cloned(),
                        });
                    }
                    if !was.same_own(c) {
                        out.push(Edit::Change(c.own()));
                    }
                    todo.push(c);
                }
                None => {
                    let node = if holds.contains(&c.id) {
                        todo.push(c);
                        c.own()
                    } else {
                        c.clone()
                    };
                    out.push(Edit::Insert {
                        parent: Some(p.id.clone()),
                        after: prev.cloned(),
                        node,
                    });
                }
            }
            prev = Some(&c.id);
        }
    }
    let kept: HashSet<&NodeId> = {
        fn ids<'a>(n: &'a Node, out: &mut HashSet<&'a NodeId>) {
            out.insert(&n.id);
            for c in &n.children {
                ids(c, out);
            }
        }
        let mut out = HashSet::new();
        ids(new, &mut out);
        out
    };
    fn gone(n: &Node, kept: &HashSet<&NodeId>, out: &mut Vec<Edit>) {
        for c in &n.children {
            if kept.contains(&c.id) {
                gone(c, kept, out);
            } else {
                out.push(Edit::Remove(c.id.clone()));
            }
        }
    }
    gone(was, &kept, &mut out);
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

/// What edits did, by id, for a drawing kept beside the scene to follow:
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

/// Applies `edits` to `scene`, whose files `uses` counts, or says why it
/// can't — leaving both as they were. The work is the edits': each finds its
/// nodes by id, and the files no node uses any more are dropped by count,
/// never by a walk of the scene.
pub fn apply(scene: &mut Scene, uses: &mut Uses, edits: &Edits) -> Result<Applied, String> {
    let (images, fonts) = (&edits.images, &edits.fonts);
    let mut undo = Vec::new();
    let mut applied = Applied::default();
    let done =
        run(scene, uses, &edits.list, &mut undo, &mut applied).and_then(|()| {
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
    scene
        .images
        .extend(images.iter().map(|(id, f)| (*id, f.clone())));
    scene
        .fonts
        .extend(fonts.iter().map(|(id, f)| (*id, f.clone())));
    uses.settle();
    scene.images.retain(|id, _| uses.images.contains_key(id));
    scene.fonts.retain(|id, _| uses.fonts.contains_key(id));
    scene.themed = edits.themed;
    Ok(applied)
}

fn run(
    scene: &mut Scene,
    uses: &mut Uses,
    ops: &[Edit],
    undo: &mut Vec<Undo>,
    applied: &mut Applied,
) -> Result<(), String> {
    for o in ops {
        match o {
            Edit::Change(new) => {
                let order = order_of(scene, &new.id)?;
                let new = new.clone();
                let place = scene.at_mut(&order).expect("indexed");
                let mut own = std::mem::replace(place, new);
                std::mem::swap(&mut place.children, &mut own.children);
                uses.take_own(&own);
                uses.add_own(place);
                applied.changed.push((own.id.clone(), own.clone()));
                undo.push(Undo::Own(order, own));
            }
            Edit::Insert {
                parent,
                after,
                node,
            } => {
                let node = node.clone();
                let Some(parent) = parent.as_ref() else {
                    // A new root: the scene, replaced.
                    if Ids::of(&node).len() != count(&node) {
                        return Err(format!("{} holds a node twice", node.id));
                    }
                    let old = std::mem::replace(&mut scene.root, node);
                    uses.take(&old);
                    uses.add(&scene.root);
                    scene.ids = Ids::of(&scene.root);
                    applied.removed.push(old.id.clone());
                    applied.placed.push(scene.root.id.clone());
                    undo.push(Undo::Root(old));
                    continue;
                };
                let parent = order_of(scene, parent)?;
                let after = after_key(scene, &parent, after.as_ref())?;
                check_unique(scene, &node, None)?;
                uses.add(&node);
                let id = node.id.clone();
                let order = place(scene, &parent, after.as_ref(), node);
                applied.placed.push(id);
                undo.push(Undo::Take(order));
            }
            Edit::Remove(id) => {
                let order = order_of(scene, id)?;
                if order.is_empty() {
                    return Err("the root can't be removed".into());
                }
                let (parent, key) = (
                    order[..order.len() - 1].to_vec(),
                    order[order.len() - 1].clone(),
                );
                let node = take(scene, &order);
                uses.take(&node);
                applied.removed.push(id.clone());
                undo.push(Undo::Put(parent, key, node));
            }
            Edit::Move { id, parent, after } => {
                let from = order_of(scene, id)?;
                if from.is_empty() {
                    return Err("the root can't be moved".into());
                }
                let parent = order_of(scene, parent)?;
                if parent.starts_with(&from) {
                    return Err(format!("{id} can't move under itself"));
                }
                let key = from[from.len() - 1].clone();
                let old_parent = from[..from.len() - 1].to_vec();
                let after = after_key(scene, &parent, after.as_ref())?;
                if after.as_ref() == Some(&key) && parent == old_parent {
                    return Err(format!("{id} can't move after itself"));
                }
                let node = take(scene, &from);
                let to = place(scene, &parent, after.as_ref(), node);
                applied.removed.push(id.clone());
                applied.placed.push(id.clone());
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
    after: Option<&NodeId>,
) -> Result<Option<OrderKey>, String> {
    let Some(id) = after else { return Ok(None) };
    let o = order_of(scene, id)?;
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
            scene.ids = Ids::of(&scene.root);
        }
    }
}

/// How many nodes of a scene show each image file and are set in each font
/// file: what lets edits drop the files no node uses any more without
/// walking the scene.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Uses {
    images: std::collections::HashMap<ImageId, usize>,
    fonts: std::collections::HashMap<FontId, usize>,
}

impl Uses {
    /// The files `root` and every node under it use.
    pub fn of(root: &Node) -> Uses {
        let mut uses = Uses::default();
        uses.add(root);
        uses
    }

    fn add(&mut self, n: &Node) {
        self.add_own(n);
        for c in &n.children {
            self.add(c);
        }
    }

    fn add_own(&mut self, n: &Node) {
        if let Some(i) = &n.image {
            *self.images.entry(i.id).or_default() += 1;
        }
        if let Some(font) = n.text.as_ref().and_then(|t| t.font) {
            *self.fonts.entry(font).or_default() += 1;
        }
    }

    fn take(&mut self, n: &Node) {
        self.take_own(n);
        for c in &n.children {
            self.take(c);
        }
    }

    fn take_own(&mut self, n: &Node) {
        let less = |count: Option<&mut usize>| {
            if let Some(c) = count {
                *c -= 1;
            }
        };
        if let Some(i) = &n.image {
            less(self.images.get_mut(&i.id));
        }
        if let Some(font) = n.text.as_ref().and_then(|t| t.font) {
            less(self.fonts.get_mut(&font));
        }
    }

    /// Drops the files no node uses any more.
    fn settle(&mut self) {
        self.images.retain(|_, c| *c > 0);
        self.fonts.retain(|_, c| *c > 0);
    }
}

#[cfg(test)]
mod tests;
