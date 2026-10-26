//! Scenes on the wire: a scene to its `proto::Scene` and back, and the patch
//! between two scenes. Each image file travels once: a scene carries every
//! one its nodes show, a patch only those the scene before it didn't have.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::proto;
use crate::resolve::{
    Align, Color, Fill, Image, ImageFile, ImageFiles, ImageId, Kind, Layout, Node, Origin, Scene,
    Shadow, Size, TextAlign, TextStyle, Vector,
};

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
                        color: rgba(*c),
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
            (Some(c), _) => Some(Fill::Solid(color(*c))),
            (None, Some(g)) => Some(Fill::Linear {
                angle: g.angle,
                stops: g.stops.iter().map(|s| (color(s.color), s.at)).collect(),
            }),
            (None, None) => None,
        },
        stroke: n.stroke.map(color),
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
        text: n.text.as_ref().map(|t| TextStyle {
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

/// Keeps only the image files `root` shows, and says if it shows one that
/// isn't there.
fn settle(root: &Node, images: &mut ImageFiles) -> Result<(), String> {
    fn shown(n: &Node, out: &mut BTreeSet<ImageId>) {
        if let Some(i) = &n.image {
            out.insert(i.id);
        }
        for c in &n.children {
            shown(c, out);
        }
    }
    let mut ids = BTreeSet::new();
    shown(root, &mut ids);
    if let Some(missing) = ids.iter().find(|id| !images.contains_key(id)) {
        return Err(format!("the stream never sent the image file {missing:?}"));
    }
    images.retain(|id, _| ids.contains(id));
    Ok(())
}

/// The scene a stream message carries; image files in `held` aren't decoded
/// again.
pub fn decode_scene(message: &proto::Scene, held: &ImageFiles) -> Result<Scene, String> {
    let root = decode(message.root.as_ref().ok_or("a scene with no root")?)?;
    let mut images = files(&message.images, held)?;
    settle(&root, &mut images)?;
    Ok(Scene {
        root,
        images,
        files: Vec::new(),
    })
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

/// Applies a patch to `scene`, or says why it can't — leaving the scene as it
/// was.
pub fn apply(scene: &mut Scene, patch: &proto::Patch) -> Result<(), String> {
    let mut next = scene.clone();
    let added = files(&patch.images, &scene.images)?;
    next.images.extend(added);
    for r in &patch.replace {
        let node = r.node.as_ref().ok_or("a replacement with no node")?;
        let at = find(&mut next.root, &r.id).ok_or_else(|| format!("no node {}", r.id))?;
        *at = decode(node)?;
    }
    settle(&next.root, &mut next.images)?;
    *scene = next;
    Ok(())
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

    use prost::Message;

    use super::*;
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
        let mut applied = decode_scene(&super::scene(&old), &ImageFiles::new()).unwrap();
        apply(&mut applied, &patch).unwrap();
        assert_eq!(applied.root, new.root);
    }

    /// Files as bytes, so a test can hold an image.
    struct Mem(HashMap<String, Vec<u8>>);

    impl Files for Mem {
        fn bytes(&self, path: &str) -> Option<Vec<u8>> {
            self.0.get(path).cloned()
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
        let mut browser = decode_scene(&super::scene(&fifty), &ImageFiles::new()).unwrap();
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
        let mut kept = decode_scene(&super::scene(&more), &ImageFiles::new()).unwrap();
        assert!(
            apply(&mut kept, &lacking)
                .unwrap_err()
                .contains("never sent")
        );
        assert_eq!(kept.root, more.root, "a refused patch leaves the scene");
    }
}
