use std::collections::HashMap;

use prost::Message;

use super::*;

/// A patch applied as a canvas applies it, its files counted afresh.
fn apply(scene: &mut Scene, patch: &proto::Patch) -> Result<Applied, String> {
    let mut uses = Uses::of(&scene.root);
    let applied = super::apply(scene, &mut uses, patch)?;
    assert_eq!(uses, Uses::of(&scene.root), "the counts follow the patch");
    Ok(applied)
}

/// A patch says what it did, by id.
#[test]
fn a_patch_says_what_it_changed() {
    let old = scene(r##"<Frame layout="column"><Text>One</Text><Text>Two</Text></Frame>"##);
    let new = scene(r##"<Frame layout="column"><Text>One</Text><Text>Too</Text></Frame>"##);
    let mut held =
        decode_scene(&super::scene(&old), (&ImageFiles::new(), &FontFiles::new())).unwrap();
    let applied = apply(&mut held, &patch(&old, &new)).unwrap();
    assert_eq!(applied.changed.len(), 1);
    assert_eq!(applied.changed[0].0, new.root.children[1].id);
    assert_eq!(applied.changed[0].1.text, old.root.children[1].text);
    assert!(applied.removed.is_empty() && applied.placed.is_empty());
    assert_eq!(held.root, new.root);
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
    let changed: Vec<NodeId> = patch
        .ops
        .iter()
        .map(|o| match o.op.as_ref().unwrap() {
            proto::op::Op::Change(n) => unnode_id(n.id.as_ref()).unwrap(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(changed, vec![new.root.children[1].children[0].id.clone()]);
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
        message < jpeg + 50 * 240,
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
