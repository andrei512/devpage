//! Scenes on the wire: a scene to its `proto::Scene` and back, and the patch
//! between two scenes. Each image file and font file travels once: a scene
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
    }
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
        children: n.children.iter().map(decode).collect::<Result<_, _>>()?,
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

/// The patch that turns `old`, as last sent, into `new`: the nodes that
/// changed and the image files `old` didn't have.
pub fn patch(old: &Scene, new: &Scene) -> proto::Patch {
    proto::Patch {
        replace: diff(&old.root, &new.root),
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
    Ok(Scene {
        root,
        images,
        fonts,
        files: Vec::new(),
        themed: message.themed,
    })
}

/// The replacements that turn `old` into `new`: the highest nodes whose own
/// attributes or number of children changed, by id.
pub fn diff(old: &Node, new: &Node) -> Vec<proto::Replace> {
    let mut out = Vec::new();
    walk(old, new, &mut vec![0], &mut |order, new| {
        out.push(proto::Replace {
            id: id(order),
            node: Some(encode(new)),
        })
    });
    out
}

/// The subtrees the patch from `old` to `new` replaces: each one's id and the
/// node it replaces — what [`crate::Drawing::update`] brings a drawing
/// across, so a drawing kept beside the last scene follows the very patch a
/// canvas gets.
pub fn replaced(old: &Node, new: &Node) -> Vec<Replaced> {
    let mut out = Vec::new();
    let mut olds = Vec::new();
    walk(old, new, &mut vec![0], &mut |order, _| {
        olds.push(order.to_vec())
    });
    for order in olds {
        let mut at = old;
        for &i in &order[1..] {
            at = &at.children[i as usize];
        }
        out.push(Replaced {
            id: order,
            old: at.clone(),
        });
    }
    out
}

/// A subtree a patch replaced: its id, as the path of child positions from
/// the root, and the node that was there.
#[derive(Debug, Clone, PartialEq)]
pub struct Replaced {
    pub id: Vec<u32>,
    pub old: Node,
}

fn id(order: &[u32]) -> String {
    order
        .iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(".")
}

/// The path of child positions an id names, from the root's `0`.
fn order(id: &str) -> Option<Vec<u32>> {
    let order: Option<Vec<u32>> = id.split('.').map(|s| s.parse().ok()).collect();
    order.filter(|o| o.first() == Some(&0))
}

fn walk(old: &Node, new: &Node, order: &mut Vec<u32>, out: &mut impl FnMut(&[u32], &Node)) {
    if old == new {
        return;
    }
    if old.children.len() != new.children.len() || !old.same_own(new) {
        out(order, new);
        return;
    }
    for (i, (o, n)) in old.children.iter().zip(&new.children).enumerate() {
        order.push(i as u32);
        walk(o, n, order, out);
        order.pop();
    }
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
        if let Some(i) = &n.image {
            *self.images.entry(i.id).or_default() += 1;
        }
        if let Some(font) = n.text.as_ref().and_then(|t| t.font) {
            *self.fonts.entry(font).or_default() += 1;
        }
        for c in &n.children {
            self.add(c);
        }
    }

    fn take(&mut self, n: &Node) {
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
        for c in &n.children {
            self.take(c);
        }
    }
}

/// Applies a patch to `scene`, whose files `uses` counts, or says why it
/// can't — leaving both as they were. Everything is checked before anything
/// changes, and the work is the patch's: the replaced subtrees and the files
/// it carries, never the rest of the scene. Returns what it replaced.
pub fn apply(
    scene: &mut Scene,
    uses: &mut Uses,
    patch: &proto::Patch,
) -> Result<Vec<Replaced>, String> {
    let images = files(&patch.images, &scene.images)?;
    let fonts = font_files(&patch.fonts, &scene.fonts)?;
    let mut nodes = Vec::new();
    for r in &patch.replace {
        let node = r.node.as_ref().ok_or("a replacement with no node")?;
        let at = order(&r.id)
            .filter(|o| found(&scene.root, o))
            .ok_or_else(|| format!("no node {}", r.id))?;
        if nodes
            .iter()
            .any(|(o, _): &(Vec<u32>, Node)| o.starts_with(&at) || at.starts_with(o))
        {
            return Err(format!("{} is replaced twice", r.id));
        }
        nodes.push((at, decode(node)?));
    }
    // Every file a new subtree uses is held or arrives with the patch.
    let mut fresh = Uses::default();
    for (_, n) in &nodes {
        fresh.add(n);
    }
    if let Some(missing) = fresh
        .images
        .keys()
        .find(|id| !scene.images.contains_key(id) && !images.contains_key(id))
    {
        return Err(format!("the stream never sent the image file {missing:?}"));
    }
    if let Some(missing) = fresh
        .fonts
        .keys()
        .find(|id| !scene.fonts.contains_key(id) && !fonts.contains_key(id))
    {
        return Err(format!("the stream never sent the font file {missing:?}"));
    }
    scene.images.extend(images);
    scene.fonts.extend(fonts);
    let mut out = Vec::new();
    for (at, node) in nodes {
        let place = find(&mut scene.root, &at).expect("found above");
        uses.add(&node);
        let old = std::mem::replace(place, node);
        uses.take(&old);
        out.push(Replaced { id: at, old });
    }
    // The files no node uses any more go.
    uses.images.retain(|_, c| *c > 0);
    uses.fonts.retain(|_, c| *c > 0);
    scene.images.retain(|id, _| uses.images.contains_key(id));
    scene.fonts.retain(|id, _| uses.fonts.contains_key(id));
    scene.themed = patch.themed;
    Ok(out)
}

fn found(root: &Node, order: &[u32]) -> bool {
    let mut at = root;
    for &i in &order[1..] {
        match at.children.get(i as usize) {
            Some(c) => at = c,
            None => return false,
        }
    }
    true
}

fn find<'a>(root: &'a mut Node, order: &[u32]) -> Option<&'a mut Node> {
    let mut at = root;
    for &i in &order[1..] {
        at = at.children.get_mut(i as usize)?;
    }
    Some(at)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use prost::Message;

    use super::*;

    /// A patch applied as a canvas applies it, its files counted afresh.
    fn apply(scene: &mut Scene, patch: &proto::Patch) -> Result<Vec<Replaced>, String> {
        let mut uses = Uses::of(&scene.root);
        let replaced = super::apply(scene, &mut uses, patch)?;
        assert_eq!(uses, Uses::of(&scene.root), "the counts follow the patch");
        Ok(replaced)
    }

    /// A patch hands back what it replaced, and one that can't be applied
    /// — a node that isn't there, a node replaced twice — changes nothing,
    /// not even the files it carried.
    #[test]
    fn a_patch_returns_what_it_replaced_or_changes_nothing() {
        let old = scene(r##"<Frame layout="column"><Text>One</Text><Text>Two</Text></Frame>"##);
        let new = scene(r##"<Frame layout="column"><Text>One</Text><Text>Too</Text></Frame>"##);
        let mut held =
            decode_scene(&super::scene(&old), (&ImageFiles::new(), &FontFiles::new())).unwrap();
        let replaced = apply(&mut held, &patch(&old, &new)).unwrap();
        assert_eq!(replaced, super::replaced(&old.root, &new.root));
        assert_eq!(replaced.len(), 1);
        assert_eq!(replaced[0].id, vec![0, 1]);
        assert_eq!(replaced[0].old, old.root.children[1]);
        assert_eq!(held.root, new.root);
        for bad in [
            vec![("0.7", &new.root.children[0])],
            vec![("0", &new.root), ("0.1", &new.root.children[1])],
            vec![("x", &new.root)],
        ] {
            let mut p = patch(&new, &old);
            p.replace = bad
                .into_iter()
                .map(|(id, n)| proto::Replace {
                    id: id.into(),
                    node: Some(encode(n)),
                })
                .collect();
            let before = held.clone();
            assert!(apply(&mut held, &p).is_err());
            assert!(held == before, "a refused patch leaves the scene");
        }
    }
    use crate::Files;

    fn scene(src: &str) -> Scene {
        let files: HashMap<String, String> = [("a.design".to_string(), src.to_string())].into();
        crate::resolve(&files, "a.design").unwrap()
    }

    #[test]
    fn a_patch_carries_only_what_changed() {
        let old = scene(
            r##"<Frame layout="column"><Text>One</Text><Frame><Text>Two</Text></Frame></Frame>"##,
        );
        let new = scene(
            r##"<Frame layout="column"><Text>One</Text><Frame><Text>Three</Text></Frame></Frame>"##,
        );
        assert_eq!(decode(&encode(&old.root)).unwrap(), old.root);
        let patch = patch(&old, &new);
        assert_eq!(
            patch
                .replace
                .iter()
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>(),
            vec!["0.1.0"]
        );
        let mut applied =
            decode_scene(&super::scene(&old), (&ImageFiles::new(), &FontFiles::new())).unwrap();
        apply(&mut applied, &patch).unwrap();
        assert_eq!(applied.root, new.root);
    }

    /// Every color travels with both its values, and a scene and a patch
    /// say whether it is themed.
    #[test]
    fn a_scene_carries_both_themes() {
        let themed = |fill: &str| {
            let files: HashMap<String, String> = [
                (
                    "themes.yml".to_string(),
                    "kind: themes\nlight:\n  a: \"#ffffff\"\ndark:\n  a: \"#000000\"\n".to_string(),
                ),
                (
                    "a.design".to_string(),
                    format!(
                        "<Frame fill={{{fill}}} stroke={{{fill}}} shadow={{{{ color: {fill} }}}}><Text color={{{fill}}}>Hi</Text><Rect fill=\"linear-gradient(0deg, theme.a 0%, #f00 100%)\" /></Frame>"
                    ),
                ),
            ]
            .into();
            crate::resolve(&files, "a.design").unwrap()
        };
        let light_only = themed("\"#ffffff\"");
        let both = themed("theme.a");
        assert!(both.themed);
        assert_eq!(decode(&encode(&both.root)).unwrap(), both.root);
        let whole = decode_scene(
            &super::scene(&light_only),
            (&ImageFiles::new(), &FontFiles::new()),
        )
        .unwrap();
        assert!(whole.themed, "the gradient names theme.a");
        let mut applied = whole;
        apply(&mut applied, &patch(&light_only, &both)).unwrap();
        assert_eq!(applied.root, both.root);
        let plain = scene("<Frame />");
        apply(&mut applied, &patch(&both, &plain)).unwrap();
        assert!(!applied.themed);
    }

    /// Files as bytes, so a test can hold an image.
    struct Mem(HashMap<String, Vec<u8>>);

    impl Files for Mem {
        fn bytes(&self, path: &str) -> Option<Vec<u8>> {
            self.0.get(path).cloned()
        }
        fn list(&self, path: &str) -> Option<Vec<String>> {
            crate::resolve::listed(self.0.keys(), path)
        }
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let mut e = ::png::Encoder::new(&mut out, w, h);
        e.set_color(::png::ColorType::Rgba);
        let mut writer = e.write_header().unwrap();
        writer
            .write_image_data(&vec![200; (4 * w * h) as usize])
            .unwrap();
        drop(writer);
        out
    }

    fn shirt() -> Vec<u8> {
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../docs/designs/checkout/shirt.jpg"
        ))
        .unwrap()
    }

    fn cards(n: usize, extra: &str) -> Mem {
        let mut page = String::from("import Card from \"./card.design\"\n<Frame layout=\"row\">");
        for _ in 0..n {
            page.push_str("<Card />");
        }
        page.push_str(extra);
        page.push_str("</Frame>");
        Mem([
            (
                "card.design".to_string(),
                br#"<Frame><Image src="shirt.jpg" width={100} height={100} /></Frame>"#.to_vec(),
            ),
            ("page.design".to_string(), page.into_bytes()),
            ("shirt.jpg".to_string(), shirt()),
            ("copy.jpg".to_string(), shirt()),
        ]
        .into())
    }

    /// One photo shown fifty times is one image file: decoded once and sent
    /// once, and a patch sends an image file only when the browser doesn't
    /// have it yet.
    #[test]
    fn an_image_file_travels_once() {
        let fifty = crate::resolve(&cards(50, ""), "page.design").unwrap();
        assert_eq!(fifty.images.len(), 1);
        let message = super::scene(&fifty).encode_to_vec().len();
        let jpeg = shirt().len();
        assert!(
            message < jpeg + 50 * 200,
            "a scene of fifty uses of a {jpeg}-byte file is {message} bytes"
        );
        // Another use of the same file, under another name too: nothing new
        // to send.
        let more = crate::resolve(
            &cards(51, r#"<Image src="copy.jpg" width={10} height={10} />"#),
            "page.design",
        )
        .unwrap();
        assert_eq!(more.images.len(), 1, "copy.jpg is shirt.jpg's bytes");
        let p = patch(&fifty, &more);
        assert!(p.images.is_empty());
        let mut browser = decode_scene(
            &super::scene(&fifty),
            (&ImageFiles::new(), &FontFiles::new()),
        )
        .unwrap();
        let held = browser.images.values().next().unwrap().clone();
        apply(&mut browser, &p).unwrap();
        assert_eq!(browser.root, more.root);
        // The browser kept the image file it had decoded.
        assert!(Arc::ptr_eq(browser.images.values().next().unwrap(), &held));
        // A patch whose nodes show a file the browser lacks carries it.
        let mut fs = cards(1, r#"<Image src="other.png" width={10} height={10} />"#);
        fs.0.insert("other.png".into(), png(2, 2));
        let other = crate::resolve(&fs, "page.design").unwrap();
        let p = patch(&more, &other);
        assert_eq!(p.images.len(), 1);
        apply(&mut browser, &p).unwrap();
        assert_eq!(browser.images.len(), 2);
        // A patch whose nodes show a file never sent is refused.
        let mut lacking = p.clone();
        lacking.images.clear();
        let mut kept = decode_scene(
            &super::scene(&more),
            (&ImageFiles::new(), &FontFiles::new()),
        )
        .unwrap();
        assert!(
            apply(&mut kept, &lacking)
                .unwrap_err()
                .contains("never sent")
        );
        assert_eq!(kept.root, more.root, "a refused patch leaves the scene");
    }

    fn font(name: &str) -> Vec<u8> {
        std::fs::read(format!(
            "{}/fonts/Inter-{name}.ttf",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    fn texts(body: &str) -> Scene {
        let fs = Mem([
            ("f/Inter-Regular.ttf".to_string(), font("Regular")),
            ("f/Inter-Bold.ttf".to_string(), font("Bold")),
            (
                "themes.yml".to_string(),
                b"kind: themes\nfonts:\n  inter: ./f\nlight: {}\ndark: {}\n".to_vec(),
            ),
            (
                "a.design".to_string(),
                format!("<Frame layout=\"column\">{body}</Frame>").into_bytes(),
            ),
        ]
        .into());
        crate::resolve(&fs, "a.design").unwrap()
    }

    /// A font file used by many texts travels once, with the scene; a patch
    /// carries one only when the browser doesn't have it yet, which keeps the
    /// one it checked; a node naming one never sent is refused.
    #[test]
    fn a_font_file_travels_once() {
        let many = texts(&r#"<Text font="inter">a</Text>"#.repeat(20));
        assert_eq!(many.fonts.len(), 1);
        let message = super::scene(&many).encode_to_vec().len();
        let size = font("Regular").len();
        assert!(
            message < size + 20 * 200,
            "{message} bytes for one {size}-byte font file"
        );
        assert_eq!(decode(&encode(&many.root)).unwrap(), many.root);
        let mut browser = decode_scene(
            &super::scene(&many),
            (&ImageFiles::new(), &FontFiles::new()),
        )
        .unwrap();
        let held = browser.fonts.values().next().unwrap().clone();
        let more = texts(&r#"<Text font="inter">a</Text>"#.repeat(21));
        let p = patch(&many, &more);
        assert!(p.fonts.is_empty());
        apply(&mut browser, &p).unwrap();
        assert!(Arc::ptr_eq(browser.fonts.values().next().unwrap(), &held));
        let bold = texts(r#"<Text font="inter">a</Text><Text font="inter" weight={700}>b</Text>"#);
        let p = patch(&more, &bold);
        assert_eq!(p.fonts.len(), 1);
        apply(&mut browser, &p).unwrap();
        assert_eq!(browser.root, bold.root);
        assert_eq!(browser.fonts.len(), 2);
        let mut lacking = patch(&more, &bold);
        lacking.fonts.clear();
        let mut kept = decode_scene(
            &super::scene(&more),
            (&ImageFiles::new(), &FontFiles::new()),
        )
        .unwrap();
        assert!(
            apply(&mut kept, &lacking)
                .unwrap_err()
                .contains("never sent the font file")
        );
        // Back to Inter: the scene no longer holds the font files.
        let plain = texts("<Text>a</Text>");
        apply(&mut browser, &patch(&bold, &plain)).unwrap();
        assert!(browser.fonts.is_empty());
    }
}
