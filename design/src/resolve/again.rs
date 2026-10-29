//! Resolving again: a design kept resolved, and each change to a file it
//! read resolved again only as far as the change can reach — in a saved
//! design file, the elements whose own attributes changed and those
//! inserted, removed or moved, in every instance of the file; for a
//! `themes.yml`, a font or an image, the nodes that read it. What was
//! resolved again is the [`Edits`] that bring the scene across: no scene is
//! compared with another.
//!
//! What stays linear is reading the saved file: parsing it, matching its
//! elements against its last parse ([`crate::ids`]) and walking the two
//! side by side to see what changed.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use super::{Caller, Instance, Record, Resolver, resolved};
use crate::Problem;
use crate::parse::{self, Child, Element, TextPart};
use crate::scene::{self, Applied, Edit, Edits, Files, Holder, Layout, Node, Scene};
use crate::tree::NodeId;

/// A design kept resolved: where each node of its scene came from, the
/// files it was made from, and the files changed since it was last valid.
/// It keeps no scene: the design's one scene is its holder's, which
/// resolving it again reads and brings across by the edits it makes.
#[derive(Debug, Clone)]
pub struct Resolution {
    path: String,
    record: Record,
    /// The files the scene was made from.
    files: Vec<String>,
    /// The files changed since the scene was valid, which the next valid
    /// resolve brings it across.
    pending: BTreeSet<String>,
    /// How many times the scene was brought across whole, by comparing it
    /// with a fresh resolve: what the tests hold to none.
    pub(crate) wholes: usize,
    /// How many elements the last update resolved again, beside reading the
    /// files it saw saved: what the tests hold to what the change reached,
    /// whatever the design's size. A whole resolve is counted by `wholes`.
    pub(crate) elements: usize,
}

/// What a change made of a design.
#[derive(Debug)]
pub enum Outcome {
    /// Its scene is as it was, and valid.
    Same,
    /// Its scene changed by these edits — what the browser is sent, as a
    /// patch — applied to it once, in its holder, as `applied` says.
    Changed(Edits, Applied),
    /// It has these problems, every one; its holder's scene is left the
    /// last valid one.
    Problems(Vec<Problem>),
}

impl Resolution {
    /// The design at `path` resolved — what to resolve it again from, and
    /// its scene, for a holder to keep — or every problem it has.
    pub fn new(files: &dyn Files, path: &str) -> Result<(Resolution, Scene), Vec<Problem>> {
        let (scene, record, read) = resolved(files, path)?;
        let resolution = Resolution {
            path: path.to_string(),
            record,
            files: read,
            pending: BTreeSet::new(),
            wholes: 0,
            elements: 0,
        };
        Ok((resolution, scene))
    }

    /// The files the design's scene is made from: the design, its imports,
    /// images, themes and fonts.
    pub fn files(&self) -> &[String] {
        &self.files
    }

    /// How many times the scene was brought across whole since it was first
    /// resolved: by a comparison with a fresh resolve, where resolving parts
    /// of it again couldn't.
    pub fn wholes(&self) -> usize {
        self.wholes
    }

    /// The design after the files `changed` — as the store names them, a
    /// folder's listing by its path and a `/` — changed: what they reach
    /// resolved again against the scene `held` holds — the design's last
    /// valid one — and the edits that bring it across applied to it there,
    /// once; or every problem the design now has, `held` left as it was. A
    /// change while the design has problems is kept, and the change that
    /// makes it valid again brings the scene across all of them.
    pub fn update(
        &mut self,
        held: &mut dyn Holder,
        files: &dyn Files,
        changed: &[String],
    ) -> Outcome {
        self.pending.extend(changed.iter().cloned());
        match self.again(held, files) {
            Ok(outcome) => {
                self.pending.clear();
                outcome
            }
            Err(()) => match resolved(files, &self.path) {
                Err(problems) => Outcome::Problems(problems),
                // The parts resolved again had a problem the whole design
                // hasn't: it is brought across whole.
                Ok((scene, record, read)) => {
                    self.pending.clear();
                    self.whole(held, scene, record, read)
                }
            },
        }
    }

    /// The scene `held` holds brought across to `scene`, resolved afresh, by
    /// the edits between the two.
    fn whole(
        &mut self,
        held: &mut dyn Holder,
        scene: Scene,
        record: Record,
        read: Vec<String>,
    ) -> Outcome {
        self.wholes += 1;
        self.record = record;
        self.files = read;
        let was_themed = held.scene().themed;
        let edits = Edits::between(held.scene(), &scene);
        if edits.list.is_empty() && was_themed == edits.themed {
            return Outcome::Same;
        }
        match held.apply(&edits) {
            Ok(applied) => Outcome::Changed(edits, applied),
            // Edits made between two scenes apply to the first; if they
            // ever didn't, the scene starts again from the new one: its
            // root replaces the held one's, whatever that holds.
            Err(_) => {
                let edits = Edits::whole(&scene);
                match held.apply(&edits) {
                    Ok(applied) => Outcome::Changed(edits, applied),
                    Err(e) => Outcome::Problems(vec![Problem {
                        file: self.path.clone(),
                        pos: parse::Pos { line: 1, col: 1 },
                        message: format!("the design engine failed on this design: {e}"),
                    }]),
                }
            }
        }
    }

    /// The pending changes resolved again against `held`'s scene and brought
    /// across; or `Err` when the design has a problem now, `held` as it was.
    fn again(&mut self, held: &mut dyn Holder, files: &dyn Files) -> Result<Outcome, ()> {
        let pending: Vec<String> = self.pending.iter().cloned().collect();
        let scene = held.scene();
        let mut w = Work {
            r: Resolver::new(files),
            files,
            res: self,
            scene,
            edits: Vec::new(),
            forget: Vec::new(),
            forget_instances: Vec::new(),
            covered: HashSet::new(),
            covered_instances: Vec::new(),
            own: HashMap::new(),
            parses: HashMap::new(),
            indexes: HashMap::new(),
            failed: false,
            whole: false,
        };
        for key in &pending {
            if w.res.record.parses.contains_key(key) {
                w.file(key);
            }
        }
        for key in &pending {
            let readers: Vec<Vec<u32>> = w
                .res
                .record
                .readers
                .get(key)
                .map(|r| r.iter().cloned().collect())
                .unwrap_or_default();
            for id in readers {
                w.reader(key, &id);
            }
        }
        let Work {
            r,
            edits,
            forget,
            forget_instances,
            parses,
            failed,
            whole,
            ..
        } = w;
        self.elements = r.elements;
        if failed || !r.problems.is_empty() {
            return Err(());
        }
        if whole {
            let (scene, record, read) = resolved(files, &self.path).map_err(|_| ())?;
            return Ok(self.whole(held, scene, record, read));
        }
        // What the parts resolved again read and made replaces what the
        // nodes they replace did. Were the edits refused, the design is
        // resolved again whole, and that record replaces this one.
        let record = &mut self.record;
        for prefix in &forget_instances {
            record.forget_instances(prefix);
        }
        for id in &forget {
            record.forget(&id.0);
            record.themed.remove(id);
        }
        record.merge(r.record);
        record.parses.extend(parses);
        let themed = !record.themed.is_empty();
        if edits.is_empty() && themed == scene.themed {
            return Ok(Outcome::Same);
        }
        let edits = Edits::onto(scene, edits, &r.images, &r.fonts, themed);
        // Applied once, where the scene is held.
        let applied = held.apply(&edits).map_err(|_| ())?;
        for file in r.read {
            if !self.files.contains(&file) {
                self.files.push(file);
            }
        }
        // Even no node changed when only whether the scene is themed did.
        Ok(Outcome::Changed(edits, applied))
    }
}

impl Record {
    /// Every instance under `prefix` — and it — gone, with what their nodes
    /// read and whether a theme reached them.
    fn forget_instances(&mut self, prefix: &[u32]) {
        let gone: Vec<Vec<u32>> = self
            .instances
            .range(prefix.to_vec()..)
            .take_while(|(k, _)| k.starts_with(prefix))
            .map(|(k, _)| k.clone())
            .collect();
        for k in gone {
            if let Some(i) = self.instances.remove(&k)
                && let Some(set) = self.by_file.get_mut(&i.file)
            {
                set.remove(&k);
            }
        }
        let read: Vec<Vec<u32>> = self
            .reads
            .range(prefix.to_vec()..)
            .take_while(|(k, _)| k.starts_with(prefix))
            .map(|(k, _)| k.clone())
            .collect();
        for k in read {
            self.forget(&k);
        }
        let themed: Vec<NodeId> = self
            .themed
            .range(NodeId(prefix.to_vec())..)
            .take_while(|k| k.0.starts_with(prefix))
            .cloned()
            .collect();
        for k in themed {
            self.themed.remove(&k);
        }
    }

    /// What `other`, made resolving parts again, recorded, added.
    fn merge(&mut self, other: Record) {
        self.instances.extend(other.instances);
        for (file, set) in other.by_file {
            self.by_file.entry(file).or_default().extend(set);
        }
        for (key, set) in other.readers {
            self.readers.entry(key).or_default().extend(set);
        }
        for (id, set) in other.reads {
            self.reads.entry(id).or_default().extend(set);
        }
        self.themed.extend(other.themed);
        self.parses.extend(other.parses);
    }
}

/// One update's work: the parts resolved again, the edits they make, and
/// what the nodes they replace recorded, to forget.
struct Work<'a, 'r> {
    r: Resolver<'a>,
    files: &'a dyn Files,
    res: &'r Resolution,
    /// The scene as it is held: what the edits are found against.
    scene: &'r Scene,
    edits: Vec<Edit>,
    /// Nodes whose record goes.
    forget: Vec<NodeId>,
    /// Instances whose record goes, with everything in them.
    forget_instances: Vec<Vec<u32>>,
    /// Nodes resolved again whole, or removed, this update: nothing more to
    /// do for them.
    covered: HashSet<NodeId>,
    covered_instances: Vec<Vec<u32>>,
    /// The own attributes of the nodes resolved again this update.
    own: HashMap<NodeId, Node>,
    /// Each design file saved, parsed anew.
    parses: HashMap<String, Arc<parse::File>>,
    /// Each file's elements by id, where in its tree each is.
    indexes: HashMap<String, HashMap<u32, Vec<usize>>>,
    /// Whether something already showed the design has a problem.
    failed: bool,
    /// Whether the design is resolved again whole.
    whole: bool,
}

/// What changed in a design file, element by element: the walk of its last
/// parse and its new one side by side.
enum Act<'p> {
    /// Its own attributes, resolved again; whether it is the file's root.
    Own(&'p Element, bool),
    /// It, with its subtree, resolved again in place of the old one.
    Whole(&'p Element, &'p Element),
    Insert {
        parent: &'p Element,
        after: Option<&'p Element>,
        el: &'p Element,
    },
    Remove(&'p Element),
    Move {
        parent: &'p Element,
        after: Option<&'p Element>,
        el: &'p Element,
    },
}

impl Work<'_, '_> {
    /// The design file `path` was saved: its new parse walked beside the last,
    /// and what changed resolved again in every instance of it.
    fn file(&mut self, path: &str) {
        let old = self.res.record.parses[path].clone();
        let Some(source) = self.files.read(path) else {
            self.failed = true;
            return;
        };
        let new = match self.files.parse(path, &source) {
            Ok(new) => new,
            Err(_) => {
                self.failed = true;
                return;
            }
        };
        self.parses.insert(path.to_string(), new.clone());
        self.indexes.remove(path);
        // The file's own problems: its imports and declared props.
        let mut scope = parse::File {
            imports: new.imports.clone(),
            root: Element {
                id: new.root.id,
                name: new.root.name.clone(),
                attrs: new.root.attrs.clone(),
                children: Vec::new(),
                pos: new.root.pos,
            },
            next_id: new.next_id,
        };
        self.r.instance = Vec::new();
        self.r.current = None;
        // What binding it records belongs to no node.
        let saved = std::mem::take(&mut self.r.record);
        self.r.bind_own(path, &mut scope.root);
        let scoped = self.r.scope(path, &scope);
        self.r.record = saved;
        let Some((imports, _)) = scoped else {
            self.failed = true;
            return;
        };
        let old_imports = import_map(path, &old);
        let props = |f: &parse::File| f.root.attrs.iter().find(|a| a.name == "props").cloned();
        let props_same = match (props(&old), props(&new)) {
            (Some(a), Some(b)) => crate::ids::same_value(&a.value, &b.value),
            (None, None) => true,
            _ => false,
        };
        let instances: Vec<Vec<u32>> = self
            .res
            .record
            .by_file
            .get(path)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        if new.root.id != old.root.id || new.root.name != old.root.name || !props_same {
            // What every element of the file is resolved in changed.
            for i in instances {
                self.whole_instance(&i);
            }
            return;
        }
        // The names whose import changed: their instances are made anew.
        let retargeted: HashSet<&str> = imports
            .keys()
            .chain(old_imports.keys())
            .filter(|n| imports.get(*n) != old_imports.get(*n))
            .map(String::as_str)
            .collect();
        let is_instance = |el: &Element| imports.contains_key(&el.name);
        let was_instance = |el: &Element| old_imports.contains_key(&el.name);
        let mut acts = Vec::new();
        let mut dup = false;
        walk(
            &old.root,
            &new.root,
            true,
            &|el| is_instance(el),
            &|el| was_instance(el),
            &retargeted,
            &mut acts,
            &mut dup,
        );
        if dup {
            self.failed = true;
            return;
        }
        for i in instances {
            if self.is_covered_instance(&i) {
                continue;
            }
            let Some(inst) = self.res.record.instances.get(&i).cloned() else {
                continue;
            };
            for act in &acts {
                self.act(&i, &inst, path, act);
            }
        }
    }

    /// One change to an element, done in the instance `i` of its file.
    fn act(&mut self, i: &[u32], inst: &Instance, path: &str, act: &Act) {
        match *act {
            Act::Own(el, root) => {
                let id = if root {
                    inst.root.clone()
                } else {
                    node_id(i, el)
                };
                self.own_again(i, inst, path, el, &id, root);
            }
            Act::Whole(el, old) => {
                let was = self.node_of(i, inst, old, true);
                self.whole_again(i, inst, path, el, &was);
            }
            Act::Insert { parent, after, el } => {
                let parent_id = self.node_of(i, inst, parent, false);
                let after = after.map(|a| self.node_of(i, inst, a, false));
                if self.covered.contains(&parent_id) {
                    return;
                }
                let layout = self.layout(&parent_id);
                let Some(node) = self.subtree(i, inst, path, el, &parent_id, layout) else {
                    return;
                };
                self.edits.push(Edit::Insert {
                    parent: Some(parent_id),
                    after,
                    node,
                });
            }
            Act::Remove(old) => {
                let id = self.node_of(i, inst, old, true);
                if self.covered.contains(&id) {
                    return;
                }
                self.drop_subtree(&id);
                self.edits.push(Edit::Remove(id));
            }
            Act::Move { parent, after, el } => {
                let id = self.node_of(i, inst, el, false);
                let parent = self.node_of(i, inst, parent, false);
                let after = after.map(|a| self.node_of(i, inst, a, false));
                self.edits.push(Edit::Move { id, parent, after });
            }
        }
    }

    /// The node an element of the instance `i` makes: its own, or — for an
    /// instance of another design — that instance's root. `old` when the
    /// element is as the last parse had it.
    fn node_of(&self, i: &[u32], inst: &Instance, el: &Element, old: bool) -> NodeId {
        if el.id == root_id(inst) {
            return inst.root.clone();
        }
        let mut key = i.to_vec();
        key.push(el.id);
        // An instance of another design: made this update, or kept.
        let made = if old {
            self.res.record.instances.get(&key)
        } else {
            self.r
                .record
                .instances
                .get(&key)
                .or_else(|| self.res.record.instances.get(&key))
        };
        match made {
            Some(child) => child.root.clone(),
            None => NodeId(key),
        }
    }

    /// The layout of the frame `parent`, as this update leaves it.
    fn layout(&self, parent: &NodeId) -> Option<Layout> {
        self.own
            .get(parent)
            .or_else(|| self.scene.node(parent))
            .map(|n| n.layout)
    }

    /// The node or instance `id`, which read the file `key`, resolved again.
    fn reader(&mut self, key: &str, id: &[u32]) {
        if self.is_covered_instance(id) {
            return;
        }
        if let Some(inst) = self.res.record.instances.get(id).cloned() {
            if key.ends_with(super::THEMES_FILE) {
                // An instance given a theme color: what its props hold.
                self.whole_instance(id);
            } else {
                // A file an instance's design imports: its imports, checked.
                let Some(file) = self.parse_of(&inst.file) else {
                    self.failed = true;
                    return;
                };
                self.r.instance = id.to_vec();
                let mut scope = (*file).clone();
                scope.root.children.clear();
                let saved = std::mem::take(&mut self.r.record);
                if self.r.scope(&inst.file, &scope).is_none() {
                    self.failed = true;
                }
                self.r.record = saved;
            }
            return;
        }
        let node = NodeId(id.to_vec());
        if self.covered.contains(&node) || self.own.contains_key(&node) {
            return;
        }
        let (prefix, el_id) = id.split_at(id.len() - 1);
        let Some(inst) = self.res.record.instances.get(prefix).cloned() else {
            return;
        };
        let Some(file) = self.parse_of(&inst.file) else {
            self.failed = true;
            return;
        };
        let Some(el) = self.element(&inst.file, &file, el_id[0]) else {
            return;
        };
        let root = node == inst.root;
        self.own_again(prefix, &inst, &inst.file.clone(), &el, &node, root);
    }

    /// The parse a design file's elements are resolved from this update.
    fn parse_of(&self, path: &str) -> Option<Arc<parse::File>> {
        self.parses
            .get(path)
            .or_else(|| self.res.record.parses.get(path))
            .cloned()
    }

    /// The element of `file`, `path`'s parse, whose id is `id`.
    fn element(&mut self, path: &str, file: &parse::File, id: u32) -> Option<Element> {
        let index = self
            .indexes
            .entry(path.to_string())
            .or_insert_with(|| index(&file.root));
        let at = index.get(&id)?;
        let mut el = &file.root;
        for &i in at {
            match &el.children[i] {
                Child::Element(e) => el = e,
                Child::Text(..) => return None,
            }
        }
        Some(shallow(el))
    }

    /// The instance `i` resolved again whole, from the element that makes it
    /// in its caller's file — or, for the design's own root, the design.
    fn whole_instance(&mut self, i: &[u32]) {
        if self.is_covered_instance(i) {
            return;
        }
        let Some((caller, el_id)) = i.split_last().map(|(l, p)| (p, *l)) else {
            self.whole = true;
            return;
        };
        let (Some(inst), Some(at)) = (
            self.res.record.instances.get(caller).cloned(),
            self.res.record.instances.get(i).cloned(),
        ) else {
            self.whole = true;
            return;
        };
        let Some(file) = self.parse_of(&inst.file) else {
            self.failed = true;
            return;
        };
        let Some(el) = self.element_whole(&inst.file, &file, el_id) else {
            // The element that made it is gone: its file's walk removes it.
            return;
        };
        self.whole_again(caller, &inst, &inst.file.clone(), &el, &at.root);
    }

    /// The element of `file` whose id is `id`, with its subtree.
    fn element_whole(&mut self, path: &str, file: &parse::File, id: u32) -> Option<Element> {
        let index = self
            .indexes
            .entry(path.to_string())
            .or_insert_with(|| index(&file.root));
        let at = index.get(&id)?;
        let mut el = &file.root;
        for &i in at {
            match &el.children[i] {
                Child::Element(e) => el = e,
                Child::Text(..) => return None,
            }
        }
        Some(el.clone())
    }

    fn is_covered_instance(&self, i: &[u32]) -> bool {
        self.covered_instances.iter().any(|p| i.starts_with(p))
    }

    /// Sets the resolver up to resolve in the instance `i`: the designs
    /// enclosing it, and how deep the node at `order` is.
    fn enter(&mut self, i: &[u32], depth: usize) {
        self.r.instance = i.to_vec();
        self.r.stack = (0..=i.len())
            .filter_map(|k| self.res.record.instances.get(&i[..k]))
            .map(|inst| inst.file.clone())
            .collect();
        self.r.depth = depth;
        self.r.current = None;
    }

    /// How deep, counting through instances, the node `id` at `order` is.
    fn depth_of(&self, id: &NodeId) -> usize {
        let order = self.scene.ids.get(id).map_or(0, |o| o.len());
        order + id.0.len() - 1
    }

    /// `el`'s own attributes — in the instance `i` of its file, `path` —
    /// resolved again: its node `id` takes them if they differ.
    fn own_again(
        &mut self,
        i: &[u32],
        inst: &Instance,
        path: &str,
        el: &Element,
        id: &NodeId,
        root: bool,
    ) {
        if self.covered.contains(id) || self.own.contains_key(id) {
            return;
        }
        let Some(was) = self.scene.node(id) else {
            return;
        };
        let parent = if root {
            inst.parent
        } else {
            let order = &self.scene.ids.get(id).expect("indexed")[..];
            let parent = self.scene.at(&order[..order.len() - 1]);
            parent
                .and_then(|p| self.own.get(&p.id).or(Some(p)))
                .map(|p| p.layout)
        };
        let depth = self.depth_of(id);
        self.enter(i, depth);
        let mut el = shallow(el);
        self.r.bind_own(path, &mut el);
        self.r.own_only = true;
        let node = self
            .r
            .element(path, &el, &inst.props, &inst.imports, root, parent);
        self.r.own_only = false;
        let Some(mut node) = node else {
            self.failed = true;
            return;
        };
        if root && let Some(caller) = &inst.caller {
            overrides(&mut self.r, caller, &mut node, inst.parent);
        }
        self.forget.push(id.clone());
        if !node.same_own(was) {
            self.edits.push(Edit::Change(node.own()));
        }
        self.own.insert(id.clone(), node);
    }

    /// `el` — in the instance `i` of its file, `path` — resolved again whole,
    /// in place of the subtree of the node `was`.
    fn whole_again(&mut self, i: &[u32], inst: &Instance, path: &str, el: &Element, was: &NodeId) {
        if self.covered.contains(was) {
            return;
        }
        let Some(order) = self.scene.ids.get(was).cloned() else {
            return;
        };
        let parent_order = &order[..order.len().saturating_sub(1)];
        let Some(parent) = (!order.is_empty())
            .then(|| self.scene.at(parent_order))
            .flatten()
        else {
            // The scene's root: the design, resolved again whole.
            self.whole = true;
            return;
        };
        let parent_id = parent.id.clone();
        let after = parent
            .children
            .key_before(order.last().expect("not the root"))
            .and_then(|k| parent.children.get(k))
            .map(|n| n.id.clone());
        let layout = self.layout(&parent_id);
        let Some(node) = self.subtree(i, inst, path, el, &parent_id, layout) else {
            return;
        };
        self.drop_subtree(was);
        if node.id == *was {
            self.edits.extend(scene::subtree(self.scene, &order, &node));
        } else {
            self.edits.push(Edit::Remove(was.clone()));
            self.edits.push(Edit::Insert {
                parent: Some(parent_id),
                after,
                node,
            });
        }
    }

    /// `el` and every element in it — in the instance `i` of its file,
    /// `path` — resolved under the node `parent`, of layout `layout`.
    fn subtree(
        &mut self,
        i: &[u32],
        inst: &Instance,
        path: &str,
        el: &Element,
        parent: &NodeId,
        layout: Option<Layout>,
    ) -> Option<Node> {
        let depth = self.depth_of(parent) + 1;
        self.enter(i, depth);
        let mut el = el.clone();
        self.r.bind_element(path, &mut el);
        let node = self
            .r
            .element(path, &el, &inst.props, &inst.imports, false, layout);
        if node.is_none() {
            self.failed = true;
        }
        node
    }

    /// The subtree of the node `id` is going: what its nodes and instances
    /// recorded goes with it.
    fn drop_subtree(&mut self, id: &NodeId) {
        let Some(node) = self.scene.node(id) else {
            return;
        };
        let mut todo = vec![node];
        while let Some(n) = todo.pop() {
            self.covered.insert(n.id.clone());
            self.forget.push(n.id.clone());
            let (prefix, _) = n.id.0.split_at(n.id.0.len() - 1);
            let instance_root = !prefix.is_empty()
                && self
                    .res
                    .record
                    .instances
                    .get(prefix)
                    .is_some_and(|inst| inst.root == n.id)
                && n.id != *id;
            if instance_root || (n.id == *id && self.is_root_of_instance(&n.id)) {
                self.forget_instances.push(prefix.to_vec());
                self.covered_instances.push(prefix.to_vec());
                // Its nodes are the instance's; marked covered as they come.
                for c in &n.children {
                    todo.push(c);
                }
                continue;
            }
            for c in &n.children {
                todo.push(c);
            }
        }
    }

    fn is_root_of_instance(&self, id: &NodeId) -> bool {
        let (prefix, _) = id.0.split_at(id.0.len() - 1);
        !prefix.is_empty()
            && self
                .res
                .record
                .instances
                .get(prefix)
                .is_some_and(|inst| inst.root == *id)
    }
}

/// The attributes an instance's caller lays over its root, applied again.
fn overrides(r: &mut Resolver, caller: &Caller, root: &mut Node, parent: Option<Layout>) {
    r.current = Some(root.id.clone());
    for (attr, by) in &caller.overrides {
        r.common(&caller.file, by, root, attr, attr.value.clone());
        r.fill_needs_layout(&caller.file, attr, &attr.value, parent);
        r.place_needs_no_layout(&caller.file, attr, parent);
    }
}

/// The element id of an instance's root.
fn root_id(inst: &Instance) -> u32 {
    *inst.root.0.last().expect("an id")
}

fn node_id(i: &[u32], el: &Element) -> NodeId {
    let mut id = i.to_vec();
    id.push(el.id);
    NodeId(id)
}

/// A design file's imports, at `path`, by name: the files they name.
fn import_map(path: &str, file: &parse::File) -> HashMap<String, String> {
    file.imports
        .iter()
        .filter_map(|i| Some((i.name.clone(), super::join(path, &i.path)?)))
        .collect()
}

/// An element without its child elements: what resolving its own attributes
/// needs — a `Text`'s text included.
fn shallow(el: &Element) -> Element {
    Element {
        id: el.id,
        name: el.name.clone(),
        attrs: el.attrs.clone(),
        children: el
            .children
            .iter()
            .filter(|c| matches!(c, Child::Text(..)))
            .cloned()
            .collect(),
        pos: el.pos,
    }
}

/// Where in `root`'s tree each element is, by id: its path of child places.
fn index(root: &Element) -> HashMap<u32, Vec<usize>> {
    let mut out = HashMap::new();
    let mut todo = vec![(root, Vec::new())];
    while let Some((el, at)) = todo.pop() {
        for (i, c) in el.children.iter().enumerate() {
            if let Child::Element(e) = c {
                let mut here = at.clone();
                here.push(i);
                todo.push((e, here));
            }
        }
        out.insert(el.id, at);
    }
    out
}

/// Whether two elements' text is the same but for where it is written.
fn same_text(a: &Element, b: &Element) -> bool {
    let texts = |e: &Element| {
        e.children
            .iter()
            .filter_map(|c| match c {
                Child::Text(parts, _) => Some(
                    parts
                        .iter()
                        .map(|p| match p {
                            TextPart::Literal(s) => (0, s.clone()),
                            TextPart::Prop(s) => (1, s.clone()),
                            TextPart::Theme(s, _) => (2, s.clone()),
                        })
                        .collect::<Vec<_>>(),
                ),
                Child::Element(_) => None,
            })
            .collect::<Vec<_>>()
    };
    texts(a) == texts(b)
}

/// Whether an element is written in another place than before, in a way a
/// node keeps: where its shadow and its image's `src` are written.
fn moved_origin(a: &Element, b: &Element) -> bool {
    let at = |e: &Element| {
        e.attrs
            .iter()
            .filter(|x| x.name == "shadow" || x.name == "src")
            .map(|x| (x.pos.line, x.pos.col))
            .collect::<Vec<_>>()
    };
    at(a) != at(b)
}

fn children(el: &Element) -> Vec<&Element> {
    el.children
        .iter()
        .filter_map(|c| match c {
            Child::Element(e) => Some(e),
            Child::Text(..) => None,
        })
        .collect()
}

/// Walks `old` and `new`, the same element in a file's last parse and its
/// new one, and says what changed: its own attributes, then its children —
/// moved, inserted, removed — each kept child walked in turn. A sibling list
/// with a key twice is a problem.
#[allow(clippy::too_many_arguments)]
fn walk<'p>(
    old: &'p Element,
    new: &'p Element,
    root: bool,
    is_instance: &dyn Fn(&Element) -> bool,
    was_instance: &dyn Fn(&Element) -> bool,
    retargeted: &HashSet<&str>,
    out: &mut Vec<Act<'p>>,
    dup: &mut bool,
) {
    let own_same = crate::ids::same_own(old, new) && same_text(old, new);
    let instance = is_instance(new) || was_instance(old);
    if !root
        && (old.name != new.name
            || is_instance(new) != was_instance(old)
            || (instance && (!own_same || retargeted.contains(new.name.as_str()))))
    {
        out.push(Act::Whole(new, old));
        return;
    }
    if !own_same || moved_origin(old, new) {
        out.push(Act::Own(new, root));
    }
    let layout = |e: &Element| e.attrs.iter().find(|a| a.name == "layout").cloned();
    let relayout = match (layout(old), layout(new)) {
        (Some(a), Some(b)) => !crate::ids::same_value(&a.value, &b.value),
        (None, None) => false,
        _ => true,
    };
    let olds = children(old);
    let news = children(new);
    // A key twice among siblings.
    let mut keys = HashSet::new();
    for c in &news {
        if let Some(k) = crate::ids::key(c)
            && !keys.insert(k)
        {
            *dup = true;
        }
    }
    let rank: HashMap<u32, usize> = olds.iter().enumerate().map(|(i, e)| (e.id, i)).collect();
    let ranks: Vec<Option<usize>> = news.iter().map(|e| rank.get(&e.id).copied()).collect();
    let stay = longest_run(&ranks);
    let by_id: HashMap<u32, &Element> = olds.iter().map(|e| (e.id, *e)).collect();
    let mut prev: Option<&Element> = None;
    for (n, c) in news.iter().enumerate() {
        match by_id.get(&c.id) {
            Some(o) => {
                if !stay[n] {
                    out.push(Act::Move {
                        parent: new,
                        after: prev,
                        el: c,
                    });
                }
                walk(o, c, false, is_instance, was_instance, retargeted, out, dup);
                // A child of a frame whose layout changed is checked against it.
                if relayout {
                    if is_instance(c) {
                        out.push(Act::Whole(c, o));
                    } else {
                        out.push(Act::Own(c, false));
                    }
                }
            }
            None => out.push(Act::Insert {
                parent: new,
                after: prev,
                el: c,
            }),
        }
        prev = Some(c);
    }
    let kept: HashSet<u32> = news.iter().map(|e| e.id).collect();
    for o in olds {
        if !kept.contains(&o.id) {
            out.push(Act::Remove(o));
        }
    }
}

/// Which of a list stay where they are: the longest increasing run of their
/// places before, `None` for those that weren't there.
fn longest_run(ranks: &[Option<usize>]) -> Vec<bool> {
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

#[cfg(test)]
mod tests;
