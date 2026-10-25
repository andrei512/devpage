//! Scenes on the wire: a scene node to its `proto::Node` and back, and the
//! patch between two scenes.

use crate::proto;
use crate::resolve::{Align, Color, Kind, Layout, Node, Shadow, Size, TextStyle};

fn rgba(c: Color) -> u32 {
    u32::from_be_bytes([c.r, c.g, c.b, c.a])
}

fn color(v: u32) -> Color {
    let [r, g, b, a] = v.to_be_bytes();
    Color { r, g, b, a }
}

fn size(s: Size) -> f32 {
    match s {
        Size::Px(v) => v,
        Size::Hug => -1.0,
        Size::Fill => -2.0,
    }
}

fn unsize(v: f32) -> Size {
    if v == -1.0 {
        Size::Hug
    } else if v == -2.0 {
        Size::Fill
    } else {
        Size::Px(v)
    }
}

fn align(a: Align) -> proto::Align {
    match a {
        Align::Start => proto::Align::Start,
        Align::Center => proto::Align::Center,
        Align::End => proto::Align::End,
        Align::SpaceBetween => proto::Align::SpaceBetween,
    }
}

fn unalign(a: i32) -> Align {
    match proto::Align::try_from(a).unwrap_or(proto::Align::Start) {
        proto::Align::Start => Align::Start,
        proto::Align::Center => Align::Center,
        proto::Align::End => Align::End,
        proto::Align::SpaceBetween => Align::SpaceBetween,
    }
}

pub fn encode(n: &Node) -> proto::Node {
    proto::Node {
        kind: match n.kind {
            Kind::Frame => proto::Kind::Frame,
            Kind::Text => proto::Kind::Text,
            Kind::Rect => proto::Kind::Rect,
            Kind::Ellipse => proto::Kind::Ellipse,
        } as i32,
        name: n.name.clone(),
        x: n.x,
        y: n.y,
        width: size(n.width),
        height: size(n.height),
        opacity: n.opacity,
        on_click: n.on_click.clone().unwrap_or_default(),
        fill: n.fill.map(rgba),
        stroke: n.stroke.map(rgba),
        stroke_width: n.stroke_width,
        radius: n.radius.to_vec(),
        shadow: n.shadow.as_ref().map(|s| proto::Shadow {
            x: s.x,
            y: s.y,
            blur: s.blur,
            spread: s.spread,
            color: rgba(s.color),
        }),
        clip: n.clip,
        layout: match n.layout {
            Layout::None => proto::Layout::None,
            Layout::Row => proto::Layout::Row,
            Layout::Column => proto::Layout::Column,
        } as i32,
        gap: n.gap,
        padding: n.padding.to_vec(),
        align: align(n.align) as i32,
        justify: align(n.justify) as i32,
        text: n.text.as_ref().map(|t| proto::Text {
            content: t.content.clone(),
            size: t.size,
            weight: t.weight as u32,
            color: rgba(t.color),
            line_height: t.line_height,
        }),
        design: n.design.clone().unwrap_or_default(),
        children: n.children.iter().map(encode).collect(),
    }
}

pub fn decode(n: &proto::Node) -> Node {
    let four = |v: &[f32]| [0, 1, 2, 3].map(|i| v.get(i).copied().unwrap_or(0.0));
    Node {
        kind: match proto::Kind::try_from(n.kind).unwrap_or(proto::Kind::Frame) {
            proto::Kind::Frame => Kind::Frame,
            proto::Kind::Text => Kind::Text,
            proto::Kind::Rect => Kind::Rect,
            proto::Kind::Ellipse => Kind::Ellipse,
        },
        name: n.name.clone(),
        x: n.x,
        y: n.y,
        width: unsize(n.width),
        height: unsize(n.height),
        opacity: n.opacity,
        on_click: Some(n.on_click.clone()).filter(|s| !s.is_empty()),
        fill: n.fill.map(color),
        stroke: n.stroke.map(color),
        stroke_width: n.stroke_width,
        radius: four(&n.radius),
        shadow: n.shadow.as_ref().map(|s| Shadow {
            x: s.x,
            y: s.y,
            blur: s.blur,
            spread: s.spread,
            color: color(s.color),
        }),
        clip: n.clip,
        layout: match proto::Layout::try_from(n.layout).unwrap_or(proto::Layout::None) {
            proto::Layout::None => Layout::None,
            proto::Layout::Row => Layout::Row,
            proto::Layout::Column => Layout::Column,
        },
        gap: n.gap,
        padding: four(&n.padding),
        align: unalign(n.align),
        justify: unalign(n.justify),
        text: n.text.as_ref().map(|t| TextStyle {
            content: t.content.clone(),
            size: t.size,
            weight: t.weight as u16,
            color: color(t.color),
            line_height: t.line_height,
        }),
        design: Some(n.design.clone()).filter(|s| !s.is_empty()),
        children: n.children.iter().map(decode).collect(),
    }
}

/// The replacements that turn `old` into `new`: the highest nodes whose own
/// attributes or number of children changed, by id.
pub fn diff(old: &Node, new: &Node) -> Vec<proto::Replace> {
    let mut out = Vec::new();
    walk(old, new, "0", &mut out);
    out
}

fn walk(old: &Node, new: &Node, id: &str, out: &mut Vec<proto::Replace>) {
    if old == new {
        return;
    }
    let same_shell = old.children.len() == new.children.len() && {
        let (mut a, mut b) = (old.clone(), new.clone());
        a.children.clear();
        b.children.clear();
        a == b
    };
    if !same_shell {
        out.push(proto::Replace {
            id: id.to_string(),
            node: Some(encode(new)),
        });
        return;
    }
    for (i, (o, n)) in old.children.iter().zip(&new.children).enumerate() {
        walk(o, n, &format!("{id}.{i}"), out);
    }
}

/// Applies replacements to a scene's root.
pub fn apply(root: &mut Node, patch: &[proto::Replace]) {
    for r in patch {
        let Some(node) = &r.node else { continue };
        if let Some(at) = find(root, &r.id) {
            *at = decode(node);
        }
    }
}

fn find<'a>(root: &'a mut Node, id: &str) -> Option<&'a mut Node> {
    let mut at = root;
    for step in id.split('.').skip(1) {
        at = at.children.get_mut(step.parse::<usize>().ok()?)?;
    }
    Some(at)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn scene(src: &str) -> Node {
        let files: HashMap<String, String> = [("a.design".to_string(), src.to_string())].into();
        crate::resolve(&files, "a.design").unwrap().root
    }

    #[test]
    fn a_patch_carries_only_what_changed() {
        let old = scene(
            r##"<Frame layout="column"><Text>One</Text><Frame><Text>Two</Text></Frame></Frame>"##,
        );
        let new = scene(
            r##"<Frame layout="column"><Text>One</Text><Frame><Text>Three</Text></Frame></Frame>"##,
        );
        assert_eq!(decode(&encode(&old)), old);
        let patch = diff(&old, &new);
        assert_eq!(
            patch.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["0.1.0"]
        );
        let mut applied = old.clone();
        apply(&mut applied, &patch);
        assert_eq!(applied, new);
    }
}
