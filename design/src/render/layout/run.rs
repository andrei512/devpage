//! Runs: auto-layout frames whose children each take their own size and are
//! set one after another — no `"fill"` child along the run, no
//! `"space-between"`. A run is a leaf of the layout tree it sits in; each of
//! its children is laid out on its own, in a tree of the run's, as the only
//! item of a one-line flex container like the run, and the run's size is
//! the sum of its children's sizes and gaps, kept in prefix sums. A child's
//! change of size is one child laid out and the sums updated in log time;
//! its siblings are not laid out again.
//!
//! What a child lays out to depends on the room the run offers it across —
//! and the run is asked for its size under several constraints as its parent
//! lays out — so the run keeps its children's sizes for each constraint it
//! was asked under, a few of them, and brings each up to date by the
//! children that changed.

use std::collections::{BTreeMap, BTreeSet};

use crate::resolve::{Align, Layout, Node};
use crate::sums::{Item, Summary, Sums};
use crate::tree::{Order, OrderKey};

use super::*;

/// Which layout tree a node is in: the drawing's own, or a run's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::render) enum Owner {
    Top,
    Run(u32),
}

/// Whether `node` is laid out as a run.
pub(in crate::render) fn is_run(node: &Node) -> bool {
    if leafy(node) || node.children.is_empty() {
        return false;
    }
    node.layout != Layout::None
}

/// How a child of a run is laid out: the one-item container it is laid out
/// in, its own layout node, whether it stretches across, and the frames in
/// it that hug their children.
pub(in crate::render) struct Child {
    pub(in crate::render) wrap: NodeId,
    pub(in crate::render) node: NodeId,
    pub(in crate::render) stretch: bool,
    /// Whether what it lays out to can depend on the room it is given —
    /// see [`sensitive`]; a child that can't is laid out once, whatever the
    /// run is asked under.
    pub(in crate::render) sens: bool,
    pub(in crate::render) hugs: Vec<(NodeId, bool, bool)>,
    /// What its layout nodes were last laid out under — they hold that
    /// layout — or `None` when it changed since.
    last: Option<Shape>,
}

/// What a child is laid out under: the room across and whether along it is
/// sized at its least, its line's breadth when definite, and the length
/// along it is held to — a stretched child keeps the length it took before
/// the line stretched it, as flexbox keeps a hypothetical main size.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Shape {
    room: AvailableSpace,
    least: bool,
    line: Option<f32>,
    basis: Option<f32>,
    /// The length along a `"fill"` child is given.
    main: Option<f32>,
}

/// What the run is asked under: the room across, when definite, that its
/// line takes — else the room it offers its children across — and whether
/// along it is sized at its least.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::render) struct Ask {
    line: Option<f32>,
    room: AvailableSpace,
    least: bool,
    /// The run's length along, less its padding, when its ask fixes it and
    /// it has `"fill"` children, which share what its other children leave
    /// of it.
    main: Option<f32>,
    /// The room along it, less its padding, when definite — what a run
    /// with `"fill"` children, its length not fixed, is sized against.
    avail: Option<f32>,
}

/// A child's size under one ask: along the run, across it as laid out, and
/// across it before the line stretched it — what the line's breadth is the
/// greatest of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::render) struct Sized {
    pub(in crate::render) along: f32,
    pub(in crate::render) across: f32,
    pub(in crate::render) own: f32,
    /// Its length along before the line was known: what a stretched child
    /// keeps.
    pub(in crate::render) first: f32,
}

#[derive(Clone, Default, Debug)]
pub(in crate::render) struct Length(pub(in crate::render) f64);

impl Summary for Length {
    fn then(&self, next: &Self) -> Self {
        Length(self.0 + next.0)
    }
}

impl Item for Sized {
    type Sum = Length;
    fn summary(&self) -> Length {
        Length(self.along as f64)
    }
}

/// The sizes under one ask of the children whose size can depend on it —
/// those that stretch across the line, and those whose size can depend on
/// the room they are given — and the line's breadth. Every other child takes
/// one size under every ask: the run's `base`.
pub(in crate::render) struct Asked {
    ask: Ask,
    pub(in crate::render) line: f32,
    sizes: Sums<OrderKey, Sized>,
    owns: BTreeMap<u32, usize>,
    /// With `"fill"` children, the run's length along as flexbox sizes the
    /// container — each fill child counting its basis, not its size.
    flexed: Option<f32>,
}

/// How many asks a run keeps the sizes for.
const ASKS: usize = 4;

/// A run's layout: its own tree of its children, how it sets them, and their
/// sizes under each ask it keeps.
pub(in crate::render) struct RunLayout {
    pub(in crate::render) tree: TaffyTree<Option<Leaf>>,
    pub(in crate::render) order: Order,
    /// The tree its leaf is in, the leaf, and — in a run's tree — the key of
    /// that run's child it is inside.
    pub(in crate::render) owner: Owner,
    pub(in crate::render) leaf: NodeId,
    pub(in crate::render) inside: Option<OrderKey>,
    pub(in crate::render) row: bool,
    /// Top, right, bottom, left.
    pub(in crate::render) padding: [f32; 4],
    pub(in crate::render) gap: f32,
    pub(in crate::render) justify: Align,
    pub(in crate::render) align: Align,
    pub(in crate::render) children: BTreeMap<OrderKey, Child>,
    stale: BTreeSet<OrderKey>,
    asked: Vec<Asked>,
    /// The size of each child whose size is the same under every ask, and
    /// their breadths before any stretch, counted.
    base: Sums<OrderKey, Sized>,
    base_owns: BTreeMap<u32, usize>,
    /// The children that stretch across, and those whose size an ask can
    /// change — the children an ask lays out.
    stretched: BTreeSet<OrderKey>,
    overs: BTreeSet<OrderKey>,
    /// The children laid out since the drawing last read the run: what it
    /// visits.
    pub(in crate::render) relaid: BTreeSet<OrderKey>,
    /// The `"fill"` children, and the tree their share of the free room is
    /// found in: a one-line flex container of a proxy for each, laid out by
    /// laying the child out in its own container, and one spacer as long as
    /// every other child and gap together — so flexbox resolves the fill
    /// children against exactly the room the others leave, over the fill
    /// children alone.
    fills: BTreeMap<OrderKey, NodeId>,
    flex: TaffyTree<Option<Leaf>>,
    flexer: NodeId,
    spacer: NodeId,
    /// Whether a child was taken out since the run was last asked.
    gone: bool,
}

/// Every run's layout, by its slot.
#[derive(Default)]
pub(in crate::render) struct Runs {
    slots: Vec<Option<RunLayout>>,
    free: Vec<u32>,
}

impl Runs {
    pub(in crate::render) fn reserve(&mut self) -> u32 {
        match self.free.pop() {
            Some(id) => id,
            None => {
                self.slots.push(None);
                (self.slots.len() - 1) as u32
            }
        }
    }

    pub(in crate::render) fn put(&mut self, id: u32, run: RunLayout) {
        self.slots[id as usize] = Some(run);
    }

    pub(in crate::render) fn take(&mut self, id: u32) -> RunLayout {
        self.slots[id as usize].take().expect("a run in its slot")
    }

    pub(in crate::render) fn get(&self, id: u32) -> &RunLayout {
        self.slots[id as usize].as_ref().expect("a run in its slot")
    }

    pub(in crate::render) fn get_mut(&mut self, id: u32) -> &mut RunLayout {
        self.slots[id as usize].as_mut().expect("a run in its slot")
    }

    /// Frees a run's slot.
    pub(in crate::render) fn free(&mut self, id: u32) {
        self.slots[id as usize] = None;
        self.free.push(id);
    }

    /// The child `key` of run `id` changed: laid out again when the run is
    /// next asked for its size, and every run around it with it.
    pub(in crate::render) fn touch(
        &mut self,
        top: &mut TaffyTree<Option<Leaf>>,
        id: u32,
        key: OrderKey,
    ) {
        let mut at = (id, key);
        loop {
            let run = self.get_mut(at.0);
            run.stale.insert(at.1.clone());
            if let Some(c) = run.children.get_mut(&at.1) {
                c.last = None;
            }
            let (owner, leaf, inside) = (run.owner, run.leaf, run.inside.clone());
            match owner {
                Owner::Top => {
                    top.mark_dirty(leaf).expect("leaf");
                    return;
                }
                Owner::Run(p) => {
                    self.get_mut(p).tree.mark_dirty(leaf).expect("leaf");
                    match inside {
                        Some(k) => at = (p, k),
                        None => return,
                    }
                }
            }
        }
    }
}

impl RunLayout {
    pub(in crate::render) fn new(
        node: &Node,
        order: Order,
        owner: Owner,
        leaf: NodeId,
        inside: Option<OrderKey>,
    ) -> RunLayout {
        let mut tree = TaffyTree::new();
        tree.disable_rounding();
        let mut flex = TaffyTree::new();
        flex.disable_rounding();
        let spacer = flex
            .new_leaf(Style {
                flex_shrink: 0.0,
                ..Style::default()
            })
            .expect("leaf");
        let flexer = flex
            .new_with_children(Style::default(), &[spacer])
            .expect("node");
        let mut run = RunLayout {
            tree,
            order,
            owner,
            leaf,
            inside,
            row: true,
            padding: [0.0; 4],
            gap: 0.0,
            justify: Align::Start,
            align: Align::Start,
            children: BTreeMap::new(),
            stale: BTreeSet::new(),
            asked: Vec::new(),
            base: Sums::new(),
            base_owns: BTreeMap::new(),
            stretched: BTreeSet::new(),
            overs: BTreeSet::new(),
            relaid: BTreeSet::new(),
            gone: false,
            fills: BTreeMap::new(),
            flex,
            flexer,
            spacer,
        };
        run.set(node);
        run
    }

    /// How `node` sets its children. Only a change of direction changes
    /// what a child lays out to — its padding changes the room an ask gives,
    /// which is another ask; its gap, `justify` and `align` only where the
    /// children are placed — so only then is every size kept dropped, and
    /// true returned.
    pub(in crate::render) fn set(&mut self, node: &Node) -> bool {
        let row = node.layout == Layout::Row;
        let changed = row != self.row;
        // Its gap, padding or `align` change the room its `"fill"` children
        // share and where the line sits: every ask is settled again.
        if node.gap != self.gap || node.padding != self.padding || node.align != self.align {
            self.gone = true;
        }
        self.row = row;
        self.padding = node.padding;
        self.gap = node.gap;
        self.justify = node.justify;
        self.align = node.align;
        if changed {
            self.asked.clear();
            self.base = Sums::new();
            self.base_owns.clear();
            let keys: Vec<OrderKey> = self.children.keys().cloned().collect();
            for k in keys {
                self.children.get_mut(&k).expect("a child").last = None;
                self.stale.insert(k);
            }
            let dir = if row {
                FlexDirection::Row
            } else {
                FlexDirection::Column
            };
            for c in self.children.values() {
                let mut style = self.tree.style(c.wrap).expect("style").clone();
                style.flex_direction = dir;
                self.tree.set_style(c.wrap, style).expect("style");
            }
        }
        changed
    }

    /// Whether the child `key` stretches across, and whether its size can
    /// depend on the room it is given: laid out again when either changed.
    pub(in crate::render) fn classify(&mut self, key: &OrderKey, stretch: bool, sens: bool) {
        let Some(c) = self.children.get_mut(key) else {
            return;
        };
        if (c.stretch, c.sens) == (stretch, sens) {
            return;
        }
        c.stretch = stretch;
        c.sens = sens;
        c.last = None;
        self.stale.insert(key.clone());
    }

    /// The child `key`'s size under `a`.
    pub(in crate::render) fn size_of(&self, a: &Asked, key: &OrderKey) -> Option<Sized> {
        if self.overs.contains(key) {
            a.sizes.get(key).copied()
        } else {
            self.base.get(key).copied()
        }
    }

    /// Adds a child, laid out when the run is next asked for its size.
    pub(in crate::render) fn add(
        &mut self,
        key: OrderKey,
        node: NodeId,
        stretch: bool,
        sens: bool,
        hugs: Vec<(NodeId, bool, bool)>,
    ) {
        let wrap = self
            .tree
            .new_with_children(self.wrap_style(), &[node])
            .expect("node");
        self.children.insert(
            key.clone(),
            Child {
                wrap,
                node,
                stretch,
                sens,
                hugs,
                last: None,
            },
        );
        self.stale.insert(key);
    }

    /// Makes the child `key` one of the run's `"fill"` children, its proxy
    /// in the flex tree styled as `style`, the child's own.
    pub(in crate::render) fn add_fill(&mut self, key: OrderKey, style: Style) {
        let proxy = self
            .flex
            .new_leaf_with_context(style, Some(Leaf::Proxy(key.clone())))
            .expect("leaf");
        self.flex.add_child(self.flexer, proxy).expect("child");
        self.fills.insert(key.clone(), proxy);
        if let Some(c) = self.children.get_mut(&key) {
            c.last = None;
        }
        self.stale.insert(key);
    }

    /// Takes a child out, its layout nodes with it.
    pub(in crate::render) fn remove(&mut self, key: &OrderKey) -> Option<Child> {
        if let Some(proxy) = self.fills.remove(key) {
            self.flex.remove_child(self.flexer, proxy).expect("child");
            let _ = self.flex.remove(proxy);
        }
        let c = self.children.remove(key)?;
        self.tree.remove_child(c.wrap, c.node).expect("child");
        let _ = self.tree.remove(c.wrap);
        self.stale.remove(key);
        self.relaid.remove(key);
        self.forget(key);
        self.gone = true;
        Some(c)
    }

    /// Takes the child `key`'s sizes out of the base and every ask, and
    /// out of the stretched and the asked-for sets.
    fn forget(&mut self, key: &OrderKey) {
        if let Some(s) = self.base.remove(key) {
            uncount(&mut self.base_owns, s.own);
        }
        for a in &mut self.asked {
            if let Some(s) = a.sizes.remove(key) {
                uncount(&mut a.owns, s.own);
            }
        }
        self.stretched.remove(key);
        self.overs.remove(key);
    }

    fn wrap_style(&self) -> Style {
        Style {
            flex_direction: if self.row {
                FlexDirection::Row
            } else {
                FlexDirection::Column
            },
            align_items: AlignItems::FLEX_START,
            ..Style::default()
        }
    }

    pub(in crate::render) fn padding_along(&self) -> (f32, f32) {
        let [t, r, b, l] = self.padding;
        if self.row { (l, r) } else { (t, b) }
    }

    pub(in crate::render) fn padding_across(&self) -> (f32, f32) {
        let [t, r, b, l] = self.padding;
        if self.row { (t, b) } else { (l, r) }
    }

    /// The sizes laid out under the ask whose line is `line` — the run's
    /// final breadth less its padding — with the line itself.
    pub(in crate::render) fn placed(&self, line: f32) -> Option<&Asked> {
        // The ask that gave the line — as taffy's final layout gives a box
        // its size — before one that found the same breadth from its
        // children: a child that stretches lays out to another size under
        // each.
        let near = |a: &&Asked| !a.ask.least && (a.line - line).abs() < 1e-4;
        self.asked
            .iter()
            .rev()
            .filter(near)
            .find(|a| a.ask.line.is_some())
            .or_else(|| self.asked.iter().rev().find(near))
    }

    /// Where a child sits across its line: by the run's `align`, or at its
    /// start when it stretches.
    pub(in crate::render) fn across(&self, line: f32, size: &Sized, stretch: bool) -> f32 {
        // The broadest child sets the line: it sits at its start, whatever
        // the last bit of its size measured under another ask.
        if stretch || (line - size.across).abs() < 1e-3 {
            return 0.0;
        }
        match self.align {
            Align::Center => (line - size.across) / 2.0,
            Align::End => line - size.across,
            _ => 0.0,
        }
    }

    /// Where the first child starts along the run from the padding: `free`
    /// room laid out by `justify`.
    pub(in crate::render) fn lead(&self, free: f32) -> f32 {
        // A run its children fill has no free room, whatever the last bit of
        // their sum.
        if free.abs() < 1e-3 {
            return 0.0;
        }
        match self.justify {
            Align::Center => free / 2.0,
            Align::End => free,
            _ => 0.0,
        }
    }

    /// The children's total length along the run under `asked`, gaps
    /// between them included.
    pub(in crate::render) fn length(&self, asked: &Asked) -> f64 {
        let n = self.children.len();
        self.base.total().0 + asked.sizes.total().0 + self.gap as f64 * n.saturating_sub(1) as f64
    }

    /// The line's breadth under `a` when its ask doesn't fix it: the
    /// broadest child's own breadth.
    fn broadest(&self, a: &Asked) -> f32 {
        greatest(&self.base_owns).max(greatest(&a.owns))
    }
}

fn own_key(v: f32) -> u32 {
    v.max(0.0).to_bits()
}

fn count(owns: &mut BTreeMap<u32, usize>, v: f32) {
    *owns.entry(own_key(v)).or_default() += 1;
}

fn uncount(owns: &mut BTreeMap<u32, usize>, v: f32) {
    let k = own_key(v);
    if let Some(n) = owns.get_mut(&k) {
        *n -= 1;
        if *n == 0 {
            owns.remove(&k);
        }
    }
}

fn greatest(owns: &BTreeMap<u32, usize>) -> f32 {
    owns.keys().next_back().map_or(0.0, |k| f32::from_bits(*k))
}

impl Renderer {
    /// The run's size — `id`, a leaf of the tree being laid out — under
    /// what its parent asks: its children laid out where they changed or
    /// were never laid out under this ask.
    pub(in crate::render) fn measure_run(
        &mut self,
        runs: &mut Runs,
        id: u32,
        known: Size<Option<f32>>,
        available: Size<AvailableSpace>,
    ) -> Size<f32> {
        let mut run = runs.take(id);
        let (pa0, pa1) = run.padding_along();
        let (pc0, pc1) = run.padding_across();
        let (known_along, known_across) = if run.row {
            (known.width, known.height)
        } else {
            (known.height, known.width)
        };
        let (room_along, room_across) = if run.row {
            (available.width, available.height)
        } else {
            (available.height, available.width)
        };
        // As taffy sizes a flex container: across, never less than its
        // padding, whatever it is given; along, as it is given.
        let known_across = known_across.map(|c| c.max(pc0 + pc1));
        let line = known_across.map(|c| (c - pc0 - pc1).max(0.0));
        let room = match line {
            Some(l) => AvailableSpace::Definite(l),
            None => match room_across {
                AvailableSpace::Definite(v) => AvailableSpace::Definite((v - pc0 - pc1).max(0.0)),
                other => other,
            },
        };
        let (main, avail) = if run.fills.is_empty() {
            (None, None)
        } else {
            (
                known_along.map(|m| (m - pa0 - pa1).max(0.0)),
                match room_along {
                    AvailableSpace::Definite(v) => Some((v - pa0 - pa1).max(0.0)),
                    _ => None,
                },
            )
        };
        let ask = Ask {
            line,
            room,
            least: room_along == AvailableSpace::MinContent,
            main,
            avail,
        };
        self.refresh(runs, &mut run, ask);
        let at = match run.asked.iter().position(|a| a.ask == ask) {
            Some(i) => {
                let a = run.asked.remove(i);
                run.asked.push(a);
                run.asked.len() - 1
            }
            None => {
                let a = self.ask(runs, &mut run, ask);
                if run.asked.len() == ASKS {
                    run.asked.remove(0);
                }
                run.asked.push(a);
                run.asked.len() - 1
            }
        };
        let a = &run.asked[at];
        let along =
            known_along.unwrap_or_else(|| pa0 + pa1 + a.flexed.unwrap_or(run.length(a) as f32));
        let across = known_across.unwrap_or(pc0 + pc1 + a.line);
        let row = run.row;
        runs.put(id, run);
        if row {
            Size {
                width: along,
                height: across,
            }
        } else {
            Size {
                width: across,
                height: along,
            }
        }
    }

    /// The children an ask can change laid out under `ask`: those that
    /// stretch, and those whose size can depend on the room.
    fn ask(&mut self, runs: &mut Runs, run: &mut RunLayout, ask: Ask) -> Asked {
        let mut a = Asked {
            ask,
            line: ask.line.unwrap_or(0.0),
            sizes: Sums::new(),
            owns: BTreeMap::new(),
            flexed: None,
        };
        let keys: Vec<OrderKey> = run
            .overs
            .iter()
            .filter(|k| !run.fills.contains_key(*k))
            .cloned()
            .collect();
        for k in &keys {
            let s = self.child(runs, run, k, ask, ask.line, None);
            run.relaid.insert(k.clone());
            count(&mut a.owns, s.own);
            a.sizes.insert(k.clone(), s);
        }
        self.settle_line(runs, run, &mut a, None);
        a
    }

    /// The line's breadth under `a` when its ask doesn't give it, and the
    /// `"fill"` children's share of the free room: then the children that
    /// stretch across it laid out again — all of them where the line moved,
    /// else only `stale`, or all with `None`.
    fn settle_line(
        &mut self,
        runs: &mut Runs,
        run: &mut RunLayout,
        a: &mut Asked,
        stale: Option<&[OrderKey]>,
    ) {
        let was = a.line;
        if !run.fills.is_empty() {
            self.flex(runs, run, a);
        } else if a.ask.line.is_none() {
            a.line = run.broadest(a);
        }
        if a.ask.line.is_none() {
            let keys: Vec<OrderKey> = match stale {
                Some(s) if a.line == was => s.to_vec(),
                _ => run.stretched.iter().cloned().collect(),
            };
            self.across_line(runs, run, a, &keys);
        }
    }

    /// The `"fill"` children's lengths under `a`: flexbox run on the fill
    /// children's proxies and one spacer as long as every other child and
    /// gap, as broad as the broadest of them — so the fill children share
    /// exactly the room the others leave, and the line's breadth is the
    /// broadest of everything. Each fill child is then laid out at its
    /// length.
    fn flex(&mut self, runs: &mut Runs, run: &mut RunLayout, a: &mut Asked) {
        let n = run.children.len();
        let mut fills_along = 0.0f64;
        for k in run.fills.keys() {
            if let Some(s) = a.sizes.get(k) {
                fills_along += s.along as f64;
            }
        }
        let others = run.base.total().0 + a.sizes.total().0 - fills_along
            + run.gap as f64 * n.saturating_sub(1) as f64;
        let broad = run.broadest(a);
        let dim = |v: Option<f32>| v.map_or(Dimension::auto(), Dimension::length);
        let (row, align) = (run.row, run.align);
        let size = |along: Dimension, across: Dimension| {
            if row {
                Size {
                    width: along,
                    height: across,
                }
            } else {
                Size {
                    width: across,
                    height: along,
                }
            }
        };
        let spacer = Style {
            flex_shrink: 0.0,
            size: size(Dimension::length(others as f32), Dimension::length(broad)),
            ..Style::default()
        };
        run.flex.set_style(run.spacer, spacer).expect("style");
        let flexer = Style {
            flex_direction: if row {
                FlexDirection::Row
            } else {
                FlexDirection::Column
            },
            align_items: match align {
                Align::Center => AlignItems::CENTER,
                Align::End => AlignItems::FLEX_END,
                _ => AlignItems::FLEX_START,
            },
            size: size(dim(a.ask.main), dim(a.ask.line)),
            ..Style::default()
        };
        run.flex.set_style(run.flexer, flexer).expect("style");
        let along_room = match a.ask.main.or(a.ask.avail) {
            Some(m) => AvailableSpace::Definite(m),
            None if a.ask.least => AvailableSpace::MinContent,
            None => AvailableSpace::MaxContent,
        };
        let across_room = match a.ask.line {
            Some(l) => AvailableSpace::Definite(l),
            None => a.ask.room,
        };
        let available = if row {
            Size {
                width: along_room,
                height: across_room,
            }
        } else {
            Size {
                width: across_room,
                height: along_room,
            }
        };
        let mut flex = std::mem::replace(&mut run.flex, TaffyTree::new());
        let flexer_id = run.flexer;
        flex.compute_layout_with_measure(flexer_id, available, |input, _id, ctx, _style| {
            let Some(Some(Leaf::Proxy(k))) = ctx else {
                return taffy::LayoutOutput::from_outer_size(Size::ZERO);
            };
            let size = self.proxy(runs, run, k, input.known_dimensions, input.available_space);
            taffy::LayoutOutput::from_outer_size(size)
        })
        .expect("layout");
        let fl = flex.unrounded_layout(flexer_id).size;
        a.flexed = Some(if row { fl.width } else { fl.height });
        if a.ask.line.is_none() {
            a.line = if row { fl.height } else { fl.width };
        }
        let proxies: Vec<(OrderKey, NodeId)> =
            run.fills.iter().map(|(k, p)| (k.clone(), *p)).collect();
        let line = a.line;
        let mut sized = Vec::new();
        for (k, proxy) in &proxies {
            let l = flex.unrounded_layout(*proxy).size;
            let (along, own) = if row {
                (l.width, l.height)
            } else {
                (l.height, l.width)
            };
            sized.push((k.clone(), along, own));
        }
        run.flex = flex;
        for (k, along, own) in sized {
            let stretch = run.children[&k].stretch;
            let mut s = self.fill_child(runs, run, &k, a.ask, along, stretch.then_some(line));
            s.own = own;
            s.first = along;
            if a.sizes.get(&k) != Some(&s) {
                run.relaid.insert(k.clone());
            }
            a.sizes.insert(k, s);
        }
    }

    /// A `"fill"` child's size under what the run's flex tree asks of its
    /// proxy: the child laid out alone in its own container, given what the
    /// proxy is given.
    fn proxy(
        &mut self,
        runs: &mut Runs,
        run: &mut RunLayout,
        key: &OrderKey,
        known: Size<Option<f32>>,
        available: Size<AvailableSpace>,
    ) -> Size<f32> {
        let (wrap, node, hugs) = {
            let c = &run.children[key];
            (c.wrap, c.node, c.hugs.clone())
        };
        let mut style = run.tree.style(wrap).expect("style").clone();
        let dim = |v: Option<f32>| v.map_or(Dimension::auto(), Dimension::length);
        style.size = Size {
            width: dim(known.width),
            height: dim(known.height),
        };
        run.tree.set_style(wrap, style).expect("style");
        self.lay_hugging(run, &hugs);
        self.settle_in(runs, &mut run.tree, wrap, available, &hugs);
        run.children.get_mut(key).expect("a child").last = None;
        run.tree.unrounded_layout(node).size
    }

    /// A `"fill"` child laid out at `along`, its share of the free room,
    /// and across `line` when it stretches.
    fn fill_child(
        &mut self,
        runs: &mut Runs,
        run: &mut RunLayout,
        key: &OrderKey,
        ask: Ask,
        along: f32,
        line: Option<f32>,
    ) -> Sized {
        let shape = Shape {
            room: ask.room,
            least: ask.least,
            line,
            basis: None,
            main: Some(along),
        };
        let (wrap, node, hugs, last) = {
            let c = &run.children[key];
            (c.wrap, c.node, c.hugs.clone(), c.last)
        };
        if last == Some(shape) {
            return self.sized(run, node);
        }
        let mut style = run.tree.style(wrap).expect("style").clone();
        let across = line.map_or(Dimension::auto(), Dimension::length);
        if run.row {
            style.size = Size {
                width: Dimension::length(along),
                height: across,
            };
        } else {
            style.size = Size {
                width: across,
                height: Dimension::length(along),
            };
        }
        run.tree.set_style(wrap, style).expect("style");
        let room = match line {
            Some(l) => AvailableSpace::Definite(l),
            None => ask.room,
        };
        let available = if run.row {
            Size {
                width: AvailableSpace::Definite(along),
                height: room,
            }
        } else {
            Size {
                width: room,
                height: AvailableSpace::Definite(along),
            }
        };
        self.lay_hugging(run, &hugs);
        self.settle_in(runs, &mut run.tree, wrap, available, &hugs);
        run.children.get_mut(key).expect("a child").last = Some(shape);
        self.sized(run, node)
    }

    /// Every frame in a child that hugs starts from no size, so the child
    /// lays out to the same boxes under every ask and after every edit.
    fn lay_hugging(&self, run: &mut RunLayout, hugs: &[(NodeId, bool, bool)]) {
        for &(h, w, hh) in hugs {
            let mut s = run.tree.style(h).expect("style").clone();
            if w {
                s.size.width = Dimension::auto();
            }
            if hh {
                s.size.height = Dimension::auto();
            }
            run.tree.set_style(h, s).expect("style");
        }
    }

    /// The children that stretch, laid out again across the line once its
    /// breadth is known, each keeping the length it first took — as flexbox
    /// keeps an item's hypothetical main size.
    fn across_line(
        &mut self,
        runs: &mut Runs,
        run: &mut RunLayout,
        a: &mut Asked,
        keys: &[OrderKey],
    ) {
        for k in keys {
            if !run.stretched.contains(k) || run.fills.contains_key(k) {
                continue;
            }
            let Some(first) = a.sizes.get(k).copied() else {
                continue;
            };
            let basis = Some(first.first);
            let mut s = self.child(runs, run, k, a.ask, Some(a.line), basis);
            run.relaid.insert(k.clone());
            s.own = first.own;
            s.first = first.first;
            a.sizes.insert(k.clone(), s);
        }
    }

    /// The child `key` of run `rid` laid out as the run places it — under
    /// the ask whose line is `line` — so its layout nodes hold the boxes the
    /// drawing reads: a cache's answer when they already do.
    pub(in crate::render) fn relay(
        &mut self,
        runs: &mut Runs,
        rid: u32,
        key: &OrderKey,
        line: f32,
    ) {
        let mut run = runs.take(rid);
        if !run.overs.contains(key) {
            // Laid out once, under any ask: its layout nodes hold its boxes.
            runs.put(rid, run);
            return;
        }
        if run.fills.contains_key(key)
            && let Some(a) = run.placed(line)
            && let Some(s) = a.sizes.get(key).copied()
        {
            let (ask, stretch) = (a.ask, run.children[key].stretch);
            self.fill_child(runs, &mut run, key, ask, s.along, stretch.then_some(line));
            runs.put(rid, run);
            return;
        }
        if let Some(a) = run.placed(line)
            && let Some(s) = a.sizes.get(key).copied()
        {
            let ask = a.ask;
            if ask.line.is_none() && run.children[key].stretch {
                self.child(runs, &mut run, key, ask, Some(line), Some(s.first));
            } else {
                self.child(runs, &mut run, key, ask, ask.line, None);
            }
        }
        runs.put(rid, run);
    }

    /// The children laid out again where they changed: one whose size no
    /// ask can change once, into the base — under `now`, the ask being
    /// answered — and any other under every ask kept; then, where a line's
    /// breadth moved, the children that stretch across it.
    fn refresh(&mut self, runs: &mut Runs, run: &mut RunLayout, now: Ask) {
        let stale: Vec<OrderKey> = std::mem::take(&mut run.stale)
            .into_iter()
            .filter(|k| run.children.contains_key(k))
            .collect();
        // A child taken out leaves no child stale but may narrow the line.
        let gone = std::mem::take(&mut run.gone);
        if stale.is_empty() && !gone {
            return;
        }
        let mut asked = std::mem::take(&mut run.asked);
        for a in &mut asked {
            for k in &stale {
                if let Some(old) = a.sizes.remove(k) {
                    uncount(&mut a.owns, old.own);
                }
            }
        }
        for k in &stale {
            if let Some(old) = run.base.remove(k) {
                uncount(&mut run.base_owns, old.own);
            }
            let c = &run.children[k];
            let (stretch, over) = (c.stretch, c.stretch || c.sens);
            if stretch {
                run.stretched.insert(k.clone());
            } else {
                run.stretched.remove(k);
            }
            run.relaid.insert(k.clone());
            if let Some(&proxy) = run.fills.get(k) {
                // What its proxy measured is gone with it.
                run.flex.mark_dirty(proxy).expect("proxy");
                run.overs.insert(k.clone());
            } else if over {
                run.overs.insert(k.clone());
                for a in &mut asked {
                    let s = self.child(runs, run, k, a.ask, a.ask.line, None);
                    count(&mut a.owns, s.own);
                    a.sizes.insert(k.clone(), s);
                }
            } else {
                run.overs.remove(k);
                let s = self.child(runs, run, k, now, now.line, None);
                count(&mut run.base_owns, s.own);
                run.base.insert(k.clone(), s);
            }
        }
        for a in &mut asked {
            self.settle_line(runs, run, a, Some(&stale));
        }
        run.asked = asked;
    }

    /// One child laid out under `ask`, across `line` when it is definite.
    fn child(
        &mut self,
        runs: &mut Runs,
        run: &mut RunLayout,
        key: &OrderKey,
        ask: Ask,
        line: Option<f32>,
        basis: Option<f32>,
    ) -> Sized {
        let shape = Shape {
            room: ask.room,
            least: ask.least,
            line,
            basis,
            main: None,
        };
        let (wrap, node, hugs, last) = {
            let c = &run.children[key];
            (c.wrap, c.node, c.hugs.clone(), c.last)
        };
        if last == Some(shape) {
            return self.sized(run, node);
        }
        // A stretched child keeps the length it took before the line
        // stretched it.
        let mut own = run.tree.style(node).expect("style").clone();
        let want = basis.map_or(Dimension::auto(), Dimension::length);
        if own.flex_basis != want {
            own.flex_basis = want;
            run.tree.set_style(node, own).expect("style");
        }
        let mut style = run.tree.style(wrap).expect("style").clone();
        let across = line.map_or(Dimension::auto(), Dimension::length);
        let changed = if run.row {
            std::mem::replace(&mut style.size.height, across) != across
        } else {
            std::mem::replace(&mut style.size.width, across) != across
        };
        if changed {
            run.tree.set_style(wrap, style).expect("style");
        }
        let along = if ask.least {
            AvailableSpace::MinContent
        } else {
            AvailableSpace::MaxContent
        };
        let room = match line {
            Some(l) => AvailableSpace::Definite(l),
            None => ask.room,
        };
        let available = if run.row {
            Size {
                width: along,
                height: room,
            }
        } else {
            Size {
                width: room,
                height: along,
            }
        };
        // Every frame in it that hugs starts from no size, so the child lays
        // out to the same boxes under every ask and after every edit.
        for &(h, w, hh) in &hugs {
            let mut s = run.tree.style(h).expect("style").clone();
            if w {
                s.size.width = Dimension::auto();
            }
            if hh {
                s.size.height = Dimension::auto();
            }
            run.tree.set_style(h, s).expect("style");
        }
        self.settle_in(runs, &mut run.tree, wrap, available, &hugs);
        run.children.get_mut(key).expect("a child").last = Some(shape);
        self.sized(run, node)
    }

    /// A child's size as its layout nodes hold it.
    fn sized(&self, run: &RunLayout, node: NodeId) -> Sized {
        let l = run.tree.unrounded_layout(node).size;
        let (along, across) = if run.row {
            (l.width, l.height)
        } else {
            (l.height, l.width)
        };
        Sized {
            along,
            across,
            own: across,
            first: along,
        }
    }
}
