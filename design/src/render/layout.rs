//! Layout: the scene as a taffy tree — flexbox restricted to Figma's auto
//! layout, no-layout frames hugging their children — and every node's box.

use crate::resolve::{self, Align, ImageFiles, Layout, Node, Scene, Size as DSize};

use super::*;
impl Renderer {
    /// Lays the scene out: every node's box, in tree order.
    pub fn layout(&mut self, scene: &Scene) -> Vec<Frame> {
        self.register(&scene.fonts);
        let mut tree: TaffyTree<Option<Leaf>> = TaffyTree::new();
        tree.disable_rounding();
        let mut hugs = Vec::new();
        let root = build(&mut tree, &scene.root, None, &mut hugs, &scene.images);
        self.settle(&mut tree, root, &hugs);
        frames_by_order(&tree, root, &scene.root)
            .into_values()
            .collect()
    }

    pub(super) fn compute(&mut self, tree: &mut TaffyTree<Option<Leaf>>, root: NodeId) {
        tree.compute_layout_with_measure(root, Size::MAX_CONTENT, |input, _id, ctx, style| {
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
    /// Lays the tree out, hugging no-layout frames to their children.
    pub(super) fn settle(
        &mut self,
        tree: &mut TaffyTree<Option<Leaf>>,
        root: NodeId,
        hugs: &[(NodeId, bool, bool)],
    ) {
        for _ in 0..=hugs.len() {
            self.compute(tree, root);
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

pub(super) fn find_node<'a>(root: &'a Node, order: &[u32]) -> &'a Node {
    let mut at = root;
    for &i in &order[1..] {
        at = &at.children[i as usize];
    }
    at
}

pub(super) fn taffy_at(tree: &TaffyTree<Option<Leaf>>, root: NodeId, path: &[u32]) -> NodeId {
    let mut at = root;
    for &i in path {
        at = tree.child_at_index(at, i as usize).expect("child");
    }
    at
}

pub(super) fn frames_by_order(
    tree: &TaffyTree<Option<Leaf>>,
    root: NodeId,
    node: &Node,
) -> std::collections::BTreeMap<Vec<u32>, Frame> {
    let mut out = std::collections::BTreeMap::new();
    pub(super) fn go(
        tree: &TaffyTree<Option<Leaf>>,
        id: NodeId,
        node: &Node,
        order: Vec<u32>,
        ox: f32,
        oy: f32,
        out: &mut std::collections::BTreeMap<Vec<u32>, Frame>,
    ) {
        let (frame, (x, y)) = placed(tree, id, (ox, oy));
        out.insert(order.clone(), frame);
        for (i, c) in node.children.iter().enumerate() {
            let mut o = order.clone();
            o.push(i as u32);
            go(
                tree,
                tree.child_at_index(id, i).expect("child"),
                c,
                o,
                x,
                y,
                out,
            );
        }
    }
    go(tree, root, node, vec![0], 0.0, 0.0, &mut out);
    out
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
}

pub(super) fn build(
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
        .map(|c| build(tree, c, Some(node), hugs, images))
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

/// A node's box in whole pixels, in the design's coordinates, and where its
/// unrounded box starts — given where its parent's does. The tree is laid
/// out unrounded, and only the boxes read are rounded, exactly as taffy's
/// own pass rounds every box: each edge at its unrounded place in the
/// design, so a box's size never depends on where it sits.
pub(super) fn placed(
    tree: &TaffyTree<Option<Leaf>>,
    id: NodeId,
    parent: (f32, f32),
) -> (Frame, (f32, f32)) {
    let l = tree.unrounded_layout(id);
    let (x, y) = (parent.0 + l.location.x, parent.1 + l.location.y);
    let frame = Frame {
        x: snap(x),
        y: snap(y),
        width: snap(x + l.size.width) - snap(x),
        height: snap(y + l.size.height) - snap(y),
    };
    (frame, (x, y))
}
