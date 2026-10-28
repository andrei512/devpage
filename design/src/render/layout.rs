//! Layout: the scene as a taffy tree — flexbox restricted to Figma's auto
//! layout, no-layout frames hugging their children — and every node's box.

use crate::resolve::{self, Align, ImageFiles, Layout, Node, Scene, Size as DSize};
use crate::tree::{Order, OrderKey};

use super::*;

mod run;
pub(super) use run::*;
impl Renderer {
    /// Lays the scene out: every node's box, in tree order — as a drawing
    /// lays it out, runs and all.
    pub fn layout(&mut self, scene: &Scene) -> Vec<Frame> {
        self.register(&scene.fonts);
        let mut l = Laid::build(self, scene);
        l.settle_top(self, &l.hugs.clone());
        l.frames(self, &scene.root).into_values().collect()
    }

    pub(super) fn compute(
        &mut self,
        runs: &mut Runs,
        tree: &mut TaffyTree<Option<Leaf>>,
        root: NodeId,
        available: Size<AvailableSpace>,
    ) {
        tree.compute_layout_with_measure(root, available, |input, _id, ctx, style| {
            let known = input.known_dimensions;
            let fixed = |d: Dimension| (!d.is_auto()).then(|| d.value());
            let known = Size {
                width: known.width.or(fixed(style.size.width)),
                height: known.height.or(fixed(style.size.height)),
            };
            let size = match ctx {
                Some(Some(Leaf::Intrinsic(w, h))) => {
                    // A vector or an image hugging keeps its own aspect.
                    let (w, h) = (*w, *h);
                    match (known.width, known.height) {
                        (Some(kw), None) => Size {
                            width: kw,
                            height: kw * h / w,
                        },
                        (None, Some(kh)) => Size {
                            width: kh * w / h,
                            height: kh,
                        },
                        (kw, kh) => Size {
                            width: kw.unwrap_or(w),
                            height: kh.unwrap_or(h),
                        },
                    }
                }
                Some(Some(Leaf::Run(id))) => {
                    // A size the run's own style gives is never less than
                    // its padding, as taffy floors a box's own size; a size
                    // its parent gives — a stretch — is taken as it is.
                    let [t, r, b, l] = runs.get(*id).padding;
                    let own = Size {
                        width: fixed(style.size.width).map(|v| v.max(l + r)),
                        height: fixed(style.size.height).map(|v| v.max(t + b)),
                    };
                    let known = Size {
                        width: input.known_dimensions.width.or(own.width),
                        height: input.known_dimensions.height.or(own.height),
                    };
                    return taffy::LayoutOutput::from_outer_size(self.measure_run(
                        runs,
                        *id,
                        known,
                        input.available_space,
                    ));
                }
                Some(Some(Leaf::Text(text, wraps))) => {
                    let width = known.width.or(match input.available_space.width {
                        AvailableSpace::Definite(w) if *wraps => Some(w),
                        _ => None,
                    });
                    let layout = self.text_layout(text, width);
                    Size {
                        width: known.width.unwrap_or(layout.width().ceil()),
                        height: known.height.unwrap_or(layout.height()),
                    }
                }
                _ => Size {
                    width: known.width.unwrap_or(0.0),
                    height: known.height.unwrap_or(0.0),
                },
            };
            taffy::LayoutOutput::from_outer_size(size)
        })
        .expect("layout");
    }
}

impl Renderer {
    /// Lays a tree out from `root` in `available`, hugging no-layout frames
    /// to their children.
    pub(super) fn settle_in(
        &mut self,
        runs: &mut Runs,
        tree: &mut TaffyTree<Option<Leaf>>,
        root: NodeId,
        available: Size<AvailableSpace>,
        hugs: &[(NodeId, bool, bool)],
    ) {
        for _ in 0..=hugs.len() {
            self.compute(runs, tree, root, available);
            let mut changed = false;
            for &(id, w, h) in hugs {
                let (mut ew, mut eh) = (0.0f32, 0.0f32);
                for child in tree.children(id).expect("children") {
                    let l = tree.unrounded_layout(child);
                    ew = ew.max(l.location.x + l.size.width);
                    eh = eh.max(l.location.y + l.size.height);
                }
                let mut style = tree.style(id).expect("style").clone();
                let mut ch = false;
                let mut set = |d: &mut Dimension, v: f32| {
                    if *d != Dimension::length(v) {
                        *d = Dimension::length(v);
                        ch = true;
                    }
                };
                if w {
                    set(&mut style.size.width, ew);
                }
                if h {
                    set(&mut style.size.height, eh);
                }
                if ch {
                    tree.set_style(id, style).expect("style");
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }
}

pub(super) fn find_node<'a>(root: &'a Node, order: &[OrderKey]) -> &'a Node {
    let mut at = root;
    for k in order {
        at = at.children.get(k).expect("a node there");
    }
    at
}

/// Removes `id` and everything under it from the layout tree.
pub(super) fn remove_subtree(tree: &mut TaffyTree<Option<Leaf>>, id: NodeId) {
    for child in tree.children(id).unwrap_or_default() {
        remove_subtree(tree, child);
    }
    let _ = tree.remove(id);
}

/// What a leaf of the layout tree measures by: a text, or a vector's or an
/// image's own size.
pub(super) enum Leaf {
    /// A text, and whether it wraps at its width.
    Text(Box<resolve::TextStyle>, bool),
    Intrinsic(f32, f32),
    /// A run, laid out by its own tree: [`Renderer::measure_run`].
    Run(u32),
}

/// A scene's layout kept in pieces: the drawing's own layout tree, a tree
/// for each run's children, and where each node is in them.
pub(super) struct Laid {
    pub(super) top: TaffyTree<Option<Leaf>>,
    pub(super) root: NodeId,
    pub(super) runs: Runs,
    /// Each node's tree and layout node, by its order.
    pub(super) nodes: BTreeMap<Order, (Owner, NodeId)>,
    /// Each run's layout, by its node's order.
    pub(super) run_of: BTreeMap<Order, u32>,
    /// The frames that hug their children in the drawing's own tree, from
    /// the build.
    pub(super) hugs: Vec<(NodeId, bool, bool)>,
}

use std::collections::BTreeMap;

impl Laid {
    /// The layout trees of a scene, not yet laid out.
    pub(super) fn build(_r: &mut Renderer, scene: &Scene) -> Laid {
        let mut top = TaffyTree::new();
        // Only the boxes read are rounded, by `placed`, as taffy would round
        // them all.
        top.disable_rounding();
        let mut l = Laid {
            top: TaffyTree::new(),
            root: NodeId::from(0u64),
            runs: Runs::default(),
            nodes: BTreeMap::new(),
            run_of: BTreeMap::new(),
            hugs: Vec::new(),
        };
        let mut hugs = Vec::new();
        let root = l.build_node(
            &mut top,
            Owner::Top,
            None,
            &scene.root,
            None,
            &mut hugs,
            &scene.images,
            Vec::new(),
        );
        l.top = top;
        l.root = root;
        l.hugs = hugs;
        l
    }

    pub(super) fn tree(&self, owner: Owner) -> &TaffyTree<Option<Leaf>> {
        match owner {
            Owner::Top => &self.top,
            Owner::Run(id) => &self.runs.get(id).tree,
        }
    }

    pub(super) fn tree_mut(&mut self, owner: Owner) -> &mut TaffyTree<Option<Leaf>> {
        match owner {
            Owner::Top => &mut self.top,
            Owner::Run(id) => &mut self.runs.get_mut(id).tree,
        }
    }

    /// The layout tree of `node` — at `order`, in `tree`, which is `owner`'s
    /// and, in a run's tree, inside that run's child `inside` — and
    /// everything under it.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_node(
        &mut self,
        tree: &mut TaffyTree<Option<Leaf>>,
        owner: Owner,
        inside: Option<OrderKey>,
        node: &Node,
        parent: Option<&Node>,
        hugs: &mut Vec<(NodeId, bool, bool)>,
        images: &ImageFiles,
        order: Order,
    ) -> NodeId {
        let style = style_of(node, parent);
        if let Some(leaf) = leaf_of(node, images) {
            let id = tree.new_leaf_with_context(style, Some(leaf)).expect("leaf");
            self.nodes.insert(order, (owner, id));
            return id;
        }
        if is_run(node) {
            let rid = self.runs.reserve();
            // Its padding stays in its style: what a box is given is never
            // less than its padding, and taffy floors it there.
            let leaf = tree
                .new_leaf_with_context(style, Some(Leaf::Run(rid)))
                .expect("leaf");
            let mut run = RunLayout::new(node, order.clone(), owner, leaf, inside);
            for (k, c) in node.children.entries() {
                self.add_child(&mut run, rid, k, c, node, images, &order);
            }
            self.runs.put(rid, run);
            self.run_of.insert(order.clone(), rid);
            self.nodes.insert(order, (owner, leaf));
            return leaf;
        }
        let children: Vec<NodeId> = node
            .children
            .entries()
            .map(|(k, c)| {
                let mut o = order.clone();
                o.push(k.clone());
                self.build_node(tree, owner, inside.clone(), c, Some(node), hugs, images, o)
            })
            .collect();
        let id = tree.new_with_children(style, &children).expect("node");
        if let Some((w, h)) = hugs_of(node, parent) {
            hugs.push((id, w, h));
        }
        self.nodes.insert(order, (owner, id));
        id
    }

    /// Builds `child`, at key `k` under the run `node` — `run`, in slot
    /// `rid`, at `order` — into the run's tree as a child of it.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn add_child(
        &mut self,
        run: &mut RunLayout,
        rid: u32,
        k: &OrderKey,
        child: &Node,
        node: &Node,
        images: &ImageFiles,
        order: &Order,
    ) {
        let mut o = order.clone();
        o.push(k.clone());
        let mut hugs = Vec::new();
        let mut tree = std::mem::replace(&mut run.tree, TaffyTree::new());
        let id = self.build_node(
            &mut tree,
            Owner::Run(rid),
            Some(k.clone()),
            child,
            Some(node),
            &mut hugs,
            images,
            o,
        );
        run.tree = tree;
        let across = if run.row { child.height } else { child.width };
        run.add(k.clone(), id, across == DSize::Fill, hugs);
    }

    /// Lays the drawing's own tree out, and every run in it that changed;
    /// `hugs` the frames in it to hug.
    pub(super) fn settle_top(&mut self, r: &mut Renderer, hugs: &[(NodeId, bool, bool)]) {
        let root = self.root;
        r.settle_in(&mut self.runs, &mut self.top, root, Size::MAX_CONTENT, hugs);
    }

    /// Where the run at `order` places each child: the run's laid-out sizes
    /// for its final breadth, and where the first child starts along it.
    pub(super) fn run_places(
        &self,
        rid: u32,
        size: Size<f32>,
    ) -> Option<(&RunLayout, &Asked, f32)> {
        let run = self.runs.get(rid);
        let (pa0, pa1) = run.padding_along();
        let (pc0, pc1) = run.padding_across();
        let (along, across) = if run.row {
            (size.width, size.height)
        } else {
            (size.height, size.width)
        };
        let line = (across - pc0 - pc1).max(0.0);
        let asked = run.placed(line)?;
        let free = along - pa0 - pa1 - RunLayout::length(asked, run.gap) as f32;
        Some((run, asked, pa0 + run.lead(free)))
    }

    /// Every node's box, by order, in the design's coordinates.
    pub(super) fn frames(&mut self, r: &mut Renderer, root: &Node) -> BTreeMap<Order, Frame> {
        let mut out = BTreeMap::new();
        let top = self.root;
        self.frames_of(
            r,
            root,
            Vec::new(),
            Owner::Top,
            top,
            Place::default(),
            true,
            &mut out,
        );
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn frames_of(
        &mut self,
        r: &mut Renderer,
        node: &Node,
        order: Order,
        owner: Owner,
        id: NodeId,
        parent: Place,
        root: bool,
        out: &mut BTreeMap<Order, Frame>,
    ) {
        let tree = self.tree(owner);
        let (frame, place) = placed(tree, id, parent, root);
        out.insert(order.clone(), frame);
        if let Some(&rid) = self.run_of.get(&order) {
            let size = tree.unrounded_layout(id).size;
            let mut before = 0.0f64;
            for (k, c) in node.children.entries() {
                let Some((run, asked, lead)) = self.run_places(rid, size) else {
                    return;
                };
                let line = asked.line;
                let Some(s) = asked.sizes.get(k).copied() else {
                    continue;
                };
                let (pc0, _) = run.padding_across();
                let stretch = run.children[k].stretch;
                let across = pc0 + run.across(line, &s, stretch);
                let item = child_place(place, run.row, lead as f64 + before, across as f64);
                let (gap, node_id) = (run.gap, run.children[k].node);
                r.relay(&mut self.runs, rid, k, line);
                let mut o = order.clone();
                o.push(k.clone());
                self.frames_of(r, c, o, Owner::Run(rid), node_id, item, false, out);
                before += s.along as f64 + gap as f64;
            }
            return;
        }
        let kids = tree.children(id).unwrap_or_default();
        for ((k, c), &child) in node.children.entries().zip(&kids) {
            let mut o = order.clone();
            o.push(k.clone());
            self.frames_of(r, c, o, owner, child, place, false, out);
        }
    }
}

/// The place a run's child is laid out from: `along` and `across` from the
/// run's node's unrounded top-left, rounded as taffy rounds a box in the
/// flow — measured from the run's node — so that the child, at the origin of
/// its own one-item tree, gets exactly the box taffy would give it.
pub(super) fn child_place(run: Place, row: bool, along: f64, across: f64) -> Place {
    let (dx, dy) = if row {
        (along, across)
    } else {
        (across, along)
    };
    let raw = (run.raw.0 as f64 + dx, run.raw.1 as f64 + dy);
    let snapd = |v: f64| (v + 0.5).floor();
    let at = (
        (run.at.0 as f64 + snapd(raw.0) - snapd(run.raw.0 as f64)) as f32,
        (run.at.1 as f64 + snapd(raw.1) - snapd(run.raw.1 as f64)) as f32,
    );
    Place {
        at,
        raw: (raw.0 as f32, raw.1 as f32),
        block: run.block,
    }
}

/// The layout tree of `node` and everything under it as one taffy tree,
/// no run laid out by its own: what taffy alone makes of a scene, for the
/// tests to hold the drawing's layout to.
#[cfg(test)]
pub(super) fn build_plain(
    tree: &mut TaffyTree<Option<Leaf>>,
    node: &Node,
    parent: Option<&Node>,
    hugs: &mut Vec<(NodeId, bool, bool)>,
    images: &ImageFiles,
) -> NodeId {
    let style = style_of(node, parent);
    if let Some(leaf) = leaf_of(node, images) {
        return tree.new_leaf_with_context(style, Some(leaf)).expect("leaf");
    }
    let children: Vec<NodeId> = node
        .children
        .iter()
        .map(|c| build_plain(tree, c, Some(node), hugs, images))
        .collect();
    let id = tree.new_with_children(style, &children).expect("node");
    if let Some((w, h)) = hugs_of(node, parent) {
        hugs.push((id, w, h));
    }
    id
}

/// Whether `node` hugs its children's extent, as a frame with no layout
/// does — and along which axes.
pub(super) fn hugs_of(node: &Node, parent: Option<&Node>) -> Option<(bool, bool)> {
    if leafy(node) {
        return None;
    }
    // A root has no parent space to fill: on its own, `"fill"` hugs.
    let hugs_by = |s: DSize| s == DSize::Hug || (parent.is_none() && s == DSize::Fill);
    let (w, h) = (hugs_by(node.width), hugs_by(node.height));
    (node.layout == Layout::None && !node.children.is_empty() && (w || h)).then_some((w, h))
}

/// Whether `node` is a leaf of the layout tree: a text, a vector or an image.
pub(super) fn leafy(node: &Node) -> bool {
    node.text.is_some() || node.vector.is_some() || node.image.is_some()
}

/// What a leaf of the layout tree measures by, for a node that is one.
pub(super) fn leaf_of(node: &Node, images: &ImageFiles) -> Option<Leaf> {
    if let Some(text) = &node.text {
        let wraps = !matches!(node.width, DSize::Hug);
        return Some(Leaf::Text(Box::new(text.clone()), wraps));
    }
    let intrinsic = match (&node.vector, &node.image) {
        (Some(v), _) => Some((v.view_box[2], v.view_box[3])),
        (_, Some(i)) => images.get(&i.id).map(|f| (f.width as f32, f.height as f32)),
        _ => None,
    };
    intrinsic.map(|(w, h)| Leaf::Intrinsic(w, h))
}

/// A node's flexbox style, which its parent's layout decides part of.
pub(super) fn style_of(node: &Node, parent: Option<&Node>) -> Style {
    let mut style = Style::default();
    let dir = parent.map(|p| p.layout);
    let dim = |s: DSize| match s {
        DSize::Px(v) => Dimension::length(v),
        _ => Dimension::auto(),
    };
    style.size = Size {
        width: dim(node.width),
        height: dim(node.height),
    };
    let (main, cross) = match dir {
        Some(Layout::Row) => (node.width, node.height),
        Some(Layout::Column) => (node.height, node.width),
        _ => (DSize::Hug, DSize::Hug),
    };
    if main == DSize::Fill {
        style.flex_grow = 1.0;
        style.flex_basis = Dimension::length(0.0);
    }
    if cross == DSize::Fill {
        style.align_self = Some(AlignSelf::STRETCH);
    }
    if dir == Some(Layout::None) {
        style.position = Position::Absolute;
        style.inset = taffy::Rect {
            left: LengthPercentageAuto::length(node.x),
            top: LengthPercentageAuto::length(node.y),
            right: LengthPercentageAuto::auto(),
            bottom: LengthPercentageAuto::auto(),
        };
    }
    style.flex_shrink = 0.0;
    match node.layout {
        Layout::Row => style.flex_direction = FlexDirection::Row,
        Layout::Column => style.flex_direction = FlexDirection::Column,
        Layout::None => {}
    }
    style.gap = Size {
        width: LengthPercentage::length(node.gap),
        height: LengthPercentage::length(node.gap),
    };
    let [t, r, b, l] = node.padding;
    style.padding = taffy::Rect {
        top: LengthPercentage::length(t),
        right: LengthPercentage::length(r),
        bottom: LengthPercentage::length(b),
        left: LengthPercentage::length(l),
    };
    style.align_items = match node.align {
        Align::Center => AlignItems::CENTER,
        Align::End => AlignItems::FLEX_END,
        _ => AlignItems::FLEX_START,
    };
    style.justify_content = match node.justify {
        Align::Center => JustifyContent::CENTER,
        Align::End => JustifyContent::FLEX_END,
        Align::SpaceBetween => JustifyContent::SPACE_BETWEEN,
        Align::Start => JustifyContent::FLEX_START,
    };
    style
}

#[cfg(test)]
mod tests;

/// Rounds to the nearest whole pixel, a half up, as taffy rounds.
pub(super) fn snap(v: f32) -> f32 {
    (v + 0.5).floor()
}

/// Where a node's parent is, for placing the node: the parent's box's top-left
/// in whole pixels, and two of the unrounded places taffy's rounding pass
/// measures from — the parent's own, and that of the nearest ancestor that is
/// a containing block for absolutely placed boxes (one placed absolutely
/// itself, or else the root).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Place {
    pub(super) at: (f32, f32),
    pub(super) raw: (f32, f32),
    pub(super) block: (f32, f32),
}

/// A node's box in whole pixels, in the design's coordinates, and the place
/// its children are measured from — given its parent's. The tree is laid
/// out unrounded, and only the boxes read are rounded, exactly as taffy's
/// own pass rounds every box: each edge at its unrounded place measured from
/// where the pass measures it — the parent for a box in the flow, the
/// containing block for one placed absolutely, which taffy visits from
/// there — so the boxes are the very ones a tree rounded whole has.
pub(super) fn placed(
    tree: &TaffyTree<Option<Leaf>>,
    id: NodeId,
    parent: Place,
    root: bool,
) -> (Frame, Place) {
    let l = tree.unrounded_layout(id);
    let absolute = tree.style(id).expect("style").position == Position::Absolute;
    let base = if absolute { parent.block } else { parent.raw };
    let (x, y) = (base.0 + l.location.x, base.1 + l.location.y);
    let frame = Frame {
        x: parent.at.0 + snap(x) - snap(base.0),
        y: parent.at.1 + snap(y) - snap(base.1),
        width: snap(x + l.size.width) - snap(x),
        height: snap(y + l.size.height) - snap(y),
    };
    let place = Place {
        at: (frame.x, frame.y),
        raw: (x, y),
        // The root is the containing block of every absolutely placed box
        // under no other.
        block: if absolute || root {
            (x, y)
        } else {
            parent.block
        },
    };
    (frame, place)
}
