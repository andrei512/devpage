//! Scenes on the wire: a scene to its `proto::Scene` and back; the patch
//! between two scenes is [`patch`]'s. Each image file and font file travels once: a scene
//! carries every one its nodes use, a patch only those the scene before it
//! didn't have.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::fonts::{FontFile, FontFiles, FontId};
use crate::proto;
use crate::resolve::{
    Align, Color, Fill, Image, ImageFile, ImageFiles, ImageId, Kind, Layout, Node, Origin, Rgba,
    Scene, Shadow, Size, TextAlign, TextStyle, Vector,
};
use crate::tree::{Children, NodeId};

fn word(c: Rgba) -> u32 {
    u32::from_be_bytes([c.r, c.g, c.b, c.a])
}

fn unword(v: u32) -> Rgba {
    let [r, g, b, a] = v.to_be_bytes();
    Rgba { r, g, b, a }
}

fn rgba(c: Color) -> proto::Color {
    proto::Color {
        light: word(c.light),
        dark: word(c.dark),
    }
}

/// A color from the wire; one the stream left out is transparent.
fn color(c: Option<proto::Color>) -> Color {
    let c = c.unwrap_or_default();
    Color {
        light: unword(c.light),
        dark: unword(c.dark),
    }
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
            Kind::Vector => proto::Kind::Vector,
            Kind::Image => proto::Kind::Image,
        } as i32,
        name: n.name.clone(),
        x: n.x,
        y: n.y,
        width: size(n.width),
        height: size(n.height),
        opacity: n.opacity,
        on_click: n.on_click.clone().unwrap_or_default(),
        fill: match &n.fill {
            Some(Fill::Solid(c)) => Some(rgba(*c)),
            _ => None,
        },
        gradient: match &n.fill {
            Some(Fill::Linear { angle, stops }) => Some(proto::Gradient {
                angle: *angle,
                stops: stops
                    .iter()
                    .map(|(c, at)| proto::Stop {
                        color: Some(rgba(*c)),
                        at: *at,
                    })
                    .collect(),
            }),
            _ => None,
        },
        vector: n.vector.as_ref().map(|v| proto::Vector {
            d: v.d.clone(),
            view_box: v.view_box.to_vec(),
        }),
        image: n.image.as_ref().map(|i| proto::Image {
            src: i.src.clone(),
            id: i.id.0.to_vec(),
        }),
        stroke: n.stroke.map(rgba),
        stroke_width: n.stroke_width,
        radius: n.radius.to_vec(),
        shadow: n.shadow.as_ref().map(|s| proto::Shadow {
            x: s.x,
            y: s.y,
            blur: s.blur,
            spread: s.spread,
            color: Some(rgba(s.color)),
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
            font: t.font.map(|id| id.0.to_vec()).unwrap_or_default(),
            content: t.content.clone(),
            size: t.size,
            weight: t.weight as u32,
            color: Some(rgba(t.color)),
            line_height: t.line_height,
            align: match t.align {
                TextAlign::Left => proto::TextAlign::Left,
                TextAlign::Center => proto::TextAlign::Center,
                TextAlign::Right => proto::TextAlign::Right,
            } as i32,
        }),
        children: n.children.iter().map(encode).collect(),
        id: Some(node_id(&n.id)),
    }
}

/// A node's own attributes for the wire: its children aside.
pub fn encode_own(n: &Node) -> proto::Node {
    let mut own = n.clone();
    own.children.clear();
    encode(&own)
}

pub(crate) fn node_id(id: &NodeId) -> proto::NodeId {
    proto::NodeId { path: id.0.clone() }
}

pub(crate) fn unnode_id(id: Option<&proto::NodeId>) -> Result<NodeId, String> {
    id.map(|i| NodeId(i.path.clone()))
        .ok_or_else(|| "a node with no id".to_string())
}

fn image_id(bytes: &[u8]) -> Result<ImageId, String> {
    bytes
        .try_into()
        .map(ImageId)
        .map_err(|_| format!("an image file id of {} bytes, not 32", bytes.len()))
}

fn font_id(bytes: &[u8]) -> Result<FontId, String> {
    bytes
        .try_into()
        .map(FontId)
        .map_err(|_| format!("a font file id of {} bytes, not 32", bytes.len()))
}

/// A node from the wire, or why it isn't one.
pub fn decode(n: &proto::Node) -> Result<Node, String> {
    let four = |v: &[f32]| [0, 1, 2, 3].map(|i| v.get(i).copied().unwrap_or(0.0));
    Ok(Node {
        id: unnode_id(n.id.as_ref())?,
        kind: match proto::Kind::try_from(n.kind).unwrap_or(proto::Kind::Frame) {
            proto::Kind::Frame => Kind::Frame,
            proto::Kind::Text => Kind::Text,
            proto::Kind::Rect => Kind::Rect,
            proto::Kind::Ellipse => Kind::Ellipse,
            proto::Kind::Vector => Kind::Vector,
            proto::Kind::Image => Kind::Image,
        },
        name: n.name.clone(),
        x: n.x,
        y: n.y,
        width: unsize(n.width),
        height: unsize(n.height),
        opacity: n.opacity,
        on_click: Some(n.on_click.clone()).filter(|s| !s.is_empty()),
        fill: match (&n.fill, &n.gradient) {
            (Some(c), _) => Some(Fill::Solid(color(Some(*c)))),
            (None, Some(g)) => Some(Fill::Linear {
                angle: g.angle,
                stops: g.stops.iter().map(|s| (color(s.color), s.at)).collect(),
            }),
            (None, None) => None,
        },
        stroke: n.stroke.map(|c| color(Some(c))),
        stroke_width: n.stroke_width,
        radius: four(&n.radius),
        shadow: n.shadow.as_ref().map(|s| Shadow {
            x: s.x,
            y: s.y,
            blur: s.blur,
            spread: s.spread,
            color: color(s.color),
            at: Origin::default(),
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
        text: match &n.text {
            Some(t) => Some(TextStyle {
                font: if t.font.is_empty() {
                    None
                } else {
                    Some(font_id(&t.font)?)
                },
                content: t.content.clone(),
                size: t.size,
                weight: t.weight as u16,
                color: color(t.color),
                line_height: t.line_height,
                align: match proto::TextAlign::try_from(t.align).unwrap_or(proto::TextAlign::Left) {
                    proto::TextAlign::Left => TextAlign::Left,
                    proto::TextAlign::Center => TextAlign::Center,
                    proto::TextAlign::Right => TextAlign::Right,
                },
            }),
            None => None,
        },
        vector: n.vector.as_ref().map(|v| Vector {
            d: v.d.clone(),
            view_box: four(&v.view_box),
        }),
        image: match &n.image {
            Some(i) => Some(Image {
                src: i.src.clone(),
                id: image_id(&i.id)?,
                at: Origin::default(),
            }),
            None => None,
        },
        children: Children::from_vec(n.children.iter().map(decode).collect::<Result<_, _>>()?),
    })
}

/// A whole scene for the wire: its tree and every image file it shows.
pub fn scene(scene: &Scene) -> proto::Scene {
    proto::Scene {
        root: Some(encode(&scene.root)),
        images: scene.images.values().map(|f| file(f)).collect(),
        fonts: scene.fonts.values().map(|f| font_file(f)).collect(),
        themed: scene.themed,
    }
}

fn font_file(f: &FontFile) -> proto::FontFile {
    proto::FontFile {
        id: f.id.0.to_vec(),
        data: f.bytes.to_vec(),
    }
}

fn file(f: &ImageFile) -> proto::ImageFile {
    proto::ImageFile {
        id: f.id.0.to_vec(),
        data: f.bytes.clone(),
    }
}

/// The image files `files` carries, decoded — each taken from `held` instead
/// when it is already there, so a file is decoded once however often it is
/// sent.
fn files(files: &[proto::ImageFile], held: &ImageFiles) -> Result<ImageFiles, String> {
    let mut out = ImageFiles::new();
    for f in files {
        let id = image_id(&f.id)?;
        let decoded = match held.get(&id) {
            Some(known) => known.clone(),
            // The server decoded the same bytes, so they decode here too.
            None => Arc::new(ImageFile::with_id(id, f.data.clone())?),
        };
        out.insert(id, decoded);
    }
    Ok(out)
}

/// The font files `files` carries, checked — each taken from `held` instead
/// when it is already there, so a file is checked once however often it is
/// sent.
fn font_files(files: &[proto::FontFile], held: &FontFiles) -> Result<FontFiles, String> {
    let mut out = FontFiles::new();
    for f in files {
        let id = font_id(&f.id)?;
        let checked = match held.get(&id) {
            Some(known) => known.clone(),
            // The server checked the same bytes, so they pass here too.
            None => Arc::new(
                FontFile::with_id(id, f.data.clone()).map_err(|e| format!("a font file {e}"))?,
            ),
        };
        out.insert(id, checked);
    }
    Ok(out)
}

/// Keeps only the image and font files `root` uses, and says if it uses one
/// that isn't there.
fn settle(root: &Node, images: &mut ImageFiles, fonts: &mut FontFiles) -> Result<(), String> {
    fn used(n: &Node, images: &mut BTreeSet<ImageId>, fonts: &mut BTreeSet<FontId>) {
        if let Some(i) = &n.image {
            images.insert(i.id);
        }
        if let Some(font) = n.text.as_ref().and_then(|t| t.font) {
            fonts.insert(font);
        }
        for c in &n.children {
            used(c, images, fonts);
        }
    }
    let (mut image_ids, mut font_ids) = (BTreeSet::new(), BTreeSet::new());
    used(root, &mut image_ids, &mut font_ids);
    if let Some(missing) = image_ids.iter().find(|id| !images.contains_key(id)) {
        return Err(format!("the stream never sent the image file {missing:?}"));
    }
    if let Some(missing) = font_ids.iter().find(|id| !fonts.contains_key(id)) {
        return Err(format!("the stream never sent the font file {missing:?}"));
    }
    images.retain(|id, _| image_ids.contains(id));
    fonts.retain(|id, _| font_ids.contains(id));
    Ok(())
}

/// The scene a stream message carries; image and font files `held` already
/// has aren't decoded or checked again.
pub fn decode_scene(
    message: &proto::Scene,
    held: (&ImageFiles, &FontFiles),
) -> Result<Scene, String> {
    let root = decode(message.root.as_ref().ok_or("a scene with no root")?)?;
    let mut images = files(&message.images, held.0)?;
    let mut fonts = font_files(&message.fonts, held.1)?;
    settle(&root, &mut images, &mut fonts)?;
    let scene = Scene::new(root, images, fonts, Vec::new(), message.themed);
    if scene.ids.len() != count(&scene.root) {
        return Err("a scene with two nodes of the same id".into());
    }
    Ok(scene)
}

fn count(n: &Node) -> usize {
    1 + n.children.iter().map(count).sum::<usize>()
}

/// How many nodes of a scene show each image file and are set in each font
/// file: what lets a patch drop the files no node uses any more without
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

mod patch;
pub use patch::{Applied, apply, bring, patch};

#[cfg(test)]
mod tests;
