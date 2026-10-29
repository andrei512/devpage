use super::*;
use crate::scene::*;

/// The files resolving the design at `path` read: what it is made from.
fn read(files: &dyn Files, path: &str) -> Vec<String> {
    let (resolution, _) = super::Resolution::new(files, path).unwrap();
    resolution.files().to_vec()
}

fn files(list: &[(&str, &str)]) -> HashMap<String, String> {
    list.iter()
        .map(|(p, s)| (p.to_string(), s.to_string()))
        .collect()
}

#[test]
fn an_instance_takes_its_props() {
    let fs = files(&[
        (
            "a/button.design",
            r##"<Frame props={{ label: "Button", fill: "#2563eb" }} fill={fill}><Text>{label}</Text></Frame>"##,
        ),
        (
            "a/screen.design",
            r#"import Button from "./button.design"
<Frame width={390}><Button label="Pay" x={10} opacity={0.5} /></Frame>"#,
        ),
    ]);
    let scene = resolve(&fs, "a/screen.design").unwrap();
    let button = &scene.root.children[0];
    assert_eq!(button.x, 10.0);
    assert_eq!(button.opacity, 0.5);
    assert_eq!(
        button.fill,
        Some(Fill::Solid(Color::fixed(Rgba {
            r: 0x25,
            g: 0x63,
            b: 0xeb,
            a: 255
        })))
    );
    assert_eq!(button.children[0].text.as_ref().unwrap().content, "Pay");
    assert_eq!(
        read(&fs, "a/screen.design"),
        ["a/screen.design", "a/button.design"]
    );
}

#[test]
fn opacity_is_between_zero_and_one() {
    let fs = files(&[
        ("dot.design", "<Frame />"),
        (
            "screen.design",
            "import Dot from \"./dot.design\"\n<Frame opacity={1}>\n  <Rect opacity={50} />\n  <Dot opacity={-0.1} />\n</Frame>",
        ),
    ]);
    let problems: Vec<String> = resolve(&fs, "screen.design")
        .unwrap_err()
        .iter()
        .map(|p| p.to_string())
        .collect();
    assert_eq!(
        problems,
        vec![
            "screen.design:3:9: opacity: expected a number from 0 to 1, found 50",
            "screen.design:4:8: opacity: expected a number from 0 to 1, found -0.1",
        ]
    );
}

fn problems_in(fs: &HashMap<String, String>, path: &str) -> Vec<String> {
    resolve(fs, path)
        .unwrap_err()
        .iter()
        .map(|p| p.to_string())
        .collect()
}

#[test]
fn a_color_with_any_character_but_hex_digits_is_a_problem() {
    // A pasted zero-width space, other non-ASCII, and plain non-hex.
    for (value, col) in [
        ("#fff\u{200b}", 13),
        ("#ffé", 13),
        ("#ff€f00", 13),
        ("#ggg", 13),
    ] {
        let fs = files(&[("a.design", &format!(r#"<Frame fill="{value}" />"#))]);
        assert_eq!(
            problems_in(&fs, "a.design"),
            vec![format!("a.design:1:8: fill: {value} is not a color")],
            "{value:?} at {col}"
        );
    }
}

#[test]
fn props_belong_to_the_root_only() {
    let fs = files(&[(
        "a.design",
        r#"<Frame props={{ a: "x" }}>
  <Frame props={{ b: "y" }} />
</Frame>"#,
    )]);
    assert_eq!(
        problems_in(&fs, "a.design"),
        vec!["a.design:2:10: props: only a design's root declares props"]
    );
}

#[test]
fn a_root_has_no_place_of_its_own() {
    let fs = files(&[
        (
            "dot.design",
            r#"<Frame x={40} y={30} width={10} height={10} />"#,
        ),
        (
            "a.design",
            r#"import Dot from "./dot.design"
<Frame width={100} height={100}>
  <Dot x={5} y={6} />
</Frame>"#,
        ),
    ]);
    let expected = [
        "dot.design:1:8: x: a design's root has no place of its own; place an instance with x and y where it is used",
        "dot.design:1:15: y: a design's root has no place of its own; place an instance with x and y where it is used",
    ];
    assert_eq!(problems_in(&fs, "dot.design"), expected);
    // Used as an instance, the root's own x and y are still a problem; the
    // instance's x and y are where it goes.
    assert_eq!(problems_in(&fs, "a.design"), expected);
    let fs = files(&[
        ("dot.design", r#"<Frame width={10} height={10} />"#),
        (
            "a.design",
            r#"import Dot from "./dot.design"
<Frame width={100} height={100}>
  <Dot x={5} y={6} />
</Frame>"#,
        ),
    ]);
    let dot = &resolve(&fs, "a.design").unwrap().root.children[0];
    assert_eq!((dot.x, dot.y), (5.0, 6.0));
}

#[test]
fn fill_needs_a_parent_with_auto_layout() {
    let fs = files(&[
        ("dot.design", r#"<Frame width="fill" />"#),
        (
            "a.design",
            r#"import Dot from "./dot.design"
<Frame width={100} height={100}>
  <Rect width="fill" height={10} />
  <Dot />
  <Dot height="fill" />
  <Frame layout="row"><Rect width="fill" /></Frame>
</Frame>"#,
        ),
    ]);
    assert_eq!(
        problems_in(&fs, "a.design"),
        vec![
            "a.design:3:9: width: \"fill\" needs a parent with layout=\"row\" or \"column\"",
            "dot.design:1:8: width: \"fill\" needs a parent with layout=\"row\" or \"column\"",
            "dot.design:1:8: width: \"fill\" needs a parent with layout=\"row\" or \"column\"",
            "a.design:5:8: height: \"fill\" needs a parent with layout=\"row\" or \"column\"",
        ]
    );
}

#[test]
fn a_tree_nests_at_most_so_deep_through_instances() {
    // Each file nests a little; through instances the tree goes too deep.
    let mut list = Vec::new();
    let depth = 40;
    for i in 0..depth {
        let body = if i + 1 < depth {
            let inner = "<Frame>".repeat(10) + "<N />" + &"</Frame>".repeat(10);
            format!("import N from \"./{}.design\"\n{inner}", i + 1)
        } else {
            "<Frame />".to_string()
        };
        list.push((format!("{i}.design"), body));
    }
    let fs: HashMap<String, String> = list.into_iter().collect();
    let found = problems_in(&fs, "0.design");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].ends_with(&format!("nested more than {MAX_DEPTH} deep")),
        "{found:?}"
    );
}

#[test]
fn every_problem_is_reported() {
    let fs = files(&[
        (
            "button.design",
            r#"<Frame props={{ label: "B" }} padding="12px"><Text>{label}</Text></Frame>"#,
        ),
        (
            "screen.design",
            "import Button from \"./button.design\"\nimport Row from \"./row.design\"\n<Frame>\n  <Button lable=\"x\" />\n  <Row />\n  <Blob />\n</Frame>",
        ),
    ]);
    let problems: Vec<String> = resolve(&fs, "screen.design")
        .unwrap_err()
        .iter()
        .map(|p| p.to_string())
        .collect();
    assert_eq!(
        problems,
        vec![
            "screen.design:2:1: import ./row.design — no such file",
            "screen.design:4:11: Button has no prop \"lable\"",
            "button.design:1:31: padding: expected pixels or an array of 2 or 4, found \"12px\"",
            "screen.design:6:3: unknown element Blob",
        ]
    );
}

#[test]
fn an_import_of_a_missing_file_is_a_problem_even_unused() {
    let fs = files(&[("cart.design", "import Row from \"./row.design\"\n<Frame />")]);
    let problems: Vec<String> = resolve(&fs, "cart.design")
        .unwrap_err()
        .iter()
        .map(|p| p.to_string())
        .collect();
    assert_eq!(
        problems,
        vec!["cart.design:1:1: import ./row.design — no such file"]
    );
}

#[test]
fn a_design_that_uses_itself() {
    let fs = files(&[(
        "a.design",
        "import A from \"./a.design\"\n<Frame><A /></Frame>",
    )]);
    let problems = resolve(&fs, "a.design").unwrap_err();
    assert_eq!(
        problems[0].to_string(),
        "a.design:2:8: a.design uses itself"
    );
}

/// Files as bytes, so a test can hold an image.
struct Mem(HashMap<String, Vec<u8>>);

impl Files for Mem {
    fn bytes(&self, path: &str) -> Option<Vec<u8>> {
        self.0.get(path).cloned()
    }
    fn list(&self, path: &str) -> Option<Vec<String>> {
        listed(self.0.keys(), path)
    }
}

/// A 1×1 PNG that decodes.
fn png() -> Vec<u8> {
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, 1, 1);
    e.set_color(png::ColorType::Rgba);
    let mut w = e.write_header().unwrap();
    w.write_image_data(&[0, 0, 0, 255]).unwrap();
    drop(w);
    out
}

/// A `key` names a node among its siblings: two the same is a problem
/// at the second, and a key is a string; on an instance it is the
/// instance's, not a prop.
#[test]
fn a_key_is_a_string_unique_among_siblings() {
    let p = problems(r#"<Frame><Rect key="a" /><Rect key="b" /><Rect key="a" /></Frame>"#);
    assert_eq!(
        p,
        vec![r#"d/a.design:1:46: key: "a" is another sibling's key too"#]
    );
    let p = problems("<Frame><Rect key={3} /></Frame>");
    assert_eq!(p, vec!["d/a.design:1:14: key: expected a string"]);
    assert!(
        problems(r#"<Frame><Rect key="a" /><Frame key="b"><Rect key="a" /></Frame></Frame>"#)
            .is_empty()
    );
    let files: HashMap<String, String> = [
        ("b.design".to_string(), "<Frame />".to_string()),
        (
            "a.design".to_string(),
            r#"import B from "./b.design"
<Frame><B key="one" /><B key="two" /></Frame>"#
                .to_string(),
        ),
    ]
    .into();
    let s = resolve(&files, "a.design").unwrap();
    // Two instances of one file: their nodes' ids differ by the instance.
    assert_ne!(s.root.children[0].id, s.root.children[1].id);
    assert_eq!(s.ids.len(), 3);
}

fn problems(source: &str) -> Vec<String> {
    let fs = Mem([
        ("d/a.design".to_string(), source.as_bytes().to_vec()),
        ("d/photo.png".to_string(), png()),
        ("d/notes.txt".to_string(), b"hi".to_vec()),
        (
            "d/b.design".to_string(),
            br#"<Frame props={{ label: "B" }}><Text>{label}</Text></Frame>"#.to_vec(),
        ),
    ]
    .into());
    match resolve(&fs, "d/a.design") {
        Ok(_) => Vec::new(),
        Err(p) => p.iter().map(|p| p.to_string()).collect(),
    }
}

#[test]
fn each_problem_names_its_place() {
    let cases = [
        (
            "<Rect />",
            "d/a.design:1:1: the root element must be a Frame, not Rect",
        ),
        (
            "<Frame props=\"x\" />",
            "d/a.design:1:8: props: expected an object of defaults",
        ),
        (
            "<Frame fill=\"red\" />",
            "d/a.design:1:8: fill: expected a color like #rrggbb",
        ),
        (
            "<Frame fill=\"#12\" />",
            "d/a.design:1:8: fill: #12 is not a color",
        ),
        (
            "<Frame fill=\"linear-gradient(90deg, #fff 0%)\" />",
            "d/a.design:1:8: fill: expected \"linear-gradient(<angle>deg, <color> <stop>%, …)\", found \"linear-gradient(90deg, #fff 0%)\"",
        ),
        (
            "<Frame width=\"wide\" />",
            "d/a.design:1:8: width: expected pixels, \"hug\" or \"fill\", found \"wide\"",
        ),
        (
            "<Frame gap=\"4\" />",
            "d/a.design:1:8: gap: expected a number, found \"4\"",
        ),
        (
            "<Frame radius={[1, 2]} />",
            "d/a.design:1:8: radius: expected pixels or an array of 4",
        ),
        (
            "<Frame layout=\"grid\" />",
            "d/a.design:1:8: layout: expected \"row\", \"column\" or \"none\"",
        ),
        (
            "<Frame align=\"space-between\" />",
            "d/a.design:1:8: align: expected \"start\", \"center\" or \"end\"",
        ),
        (
            "<Frame clip=\"yes\" />",
            "d/a.design:1:8: clip: expected true or false",
        ),
        (
            "<Frame shadow={{ blurr: 4 }} />",
            "d/a.design:1:8: shadow: a shadow has no blurr",
        ),
        (
            "<Frame onClick=\"go ./b.design\" />",
            "d/a.design:1:8: onClick: expected \"navigate <path>\", \"back\", \"overlay <path>\" or \"close\"",
        ),
        (
            "<Frame size={4} />",
            "d/a.design:1:8: Frame takes no attribute size",
        ),
        (
            "<Frame><Text fill=\"#fff\">a</Text></Frame>",
            "d/a.design:1:14: Text takes no attribute fill",
        ),
        (
            "<Frame><Text weight={450}>a</Text></Frame>",
            "d/a.design:1:14: weight: expected 100 to 900 in steps of 100",
        ),
        (
            "<Frame><Text textAlign=\"justify\">a</Text></Frame>",
            "d/a.design:1:14: textAlign: expected \"left\", \"center\" or \"right\"",
        ),
        (
            "<Frame><Text><Rect /></Text></Frame>",
            "d/a.design:1:14: Text holds text only",
        ),
        (
            "<Frame>hello</Frame>",
            "d/a.design:1:8: text belongs in a Text, not a Frame",
        ),
        (
            "<Frame><Rect><Rect /></Rect></Frame>",
            "d/a.design:1:14: Rect takes no children",
        ),
        (
            "<Frame><Text>{nope}</Text></Frame>",
            "d/a.design:1:14: no text prop \"nope\"",
        ),
        (
            "<Frame gap={nope} />",
            "d/a.design:1:8: gap: no prop \"nope\"",
        ),
        (
            "<Frame><Vector viewBox=\"0 0 24 24\" /></Frame>",
            "d/a.design:1:8: Vector needs a path, d=\"M…\"",
        ),
        (
            "<Frame><Vector d=\"M0 0 Q\" /></Frame>",
            "d/a.design:1:16: d: not SVG path data: Unexpected EOF",
        ),
        (
            "<Frame><Vector d=\"M0 0L1 1\" viewBox=\"0 0 24\" /></Frame>",
            "d/a.design:1:29: viewBox: expected \"minX minY width height\"",
        ),
        (
            "<Frame><Image /></Frame>",
            "d/a.design:1:8: Image needs a file, src=\"…\"",
        ),
        (
            "<Frame><Image src=\"./gone.png\" /></Frame>",
            "d/a.design:1:15: src: ./gone.png — no such file",
        ),
        (
            "<Frame><Image src=\"../../x.png\" /></Frame>",
            "d/a.design:1:15: src: ../../x.png — outside the served folder",
        ),
        (
            "<Frame><Image src=\"./notes.txt\" /></Frame>",
            "d/a.design:1:15: src: ./notes.txt is not a PNG or JPEG file",
        ),
        (
            "<Frame><Rect clip /></Frame>",
            "d/a.design:1:14: Rect takes no attribute clip",
        ),
        (
            "<Frame fill=\"linear-gradient(2000000deg, #fff 0%, #000 100%)\" />",
            "d/a.design:1:8: fill: `2000000` is too large: numbers go up to 1000000",
        ),
        (
            "<Frame fill=\"linear-gradient(90deg, #fff -2000000%, #000 100%)\" />",
            "d/a.design:1:8: fill: `-2000000` is too small: numbers go down to -1000000",
        ),
        (
            "<Frame fill=\"linear-gradient(NaNdeg, #fff 0%, #000 100%)\" />",
            "d/a.design:1:8: fill: `NaN` is not a number",
        ),
        (
            "<Frame fill=\"linear-gradient(90deg, #fff 0%, #000 inf%)\" />",
            "d/a.design:1:8: fill: `inf` is not a number",
        ),
        (
            "<Frame><Vector d=\"M0 0L1 1\" viewBox=\"0 0 2000000 24\" /></Frame>",
            "d/a.design:1:29: viewBox: `2000000` is too large: numbers go up to 1000000",
        ),
        (
            "<Frame><Vector d=\"M0 0L1 1\" viewBox=\"0 0 inf 24\" /></Frame>",
            "d/a.design:1:29: viewBox: `inf` is not a number",
        ),
        (
            "<Frame><Vector d=\"M0 0L2000000 0\" /></Frame>",
            "d/a.design:1:16: d: `2000000` is too large: numbers go up to 1000000",
        ),
        (
            "<Frame><Vector d=\"M0 0L1e999 0\" /></Frame>",
            "d/a.design:1:16: d: `1e999` is too large: numbers go up to 1000000",
        ),
        (
            "<Frame><Vector d=\"M0 0L-3e6 0\" /></Frame>",
            "d/a.design:1:16: d: `-3e6` is too small: numbers go down to -1000000",
        ),
        (
            "import X from \"../../x.design\"\n<Frame />",
            "d/a.design:1:1: import ../../x.design — outside the served folder",
        ),
        (
            "import B from \"./b.design\"\n<Frame><B fill=\"#fff\" /></Frame>",
            "d/a.design:2:11: B has no prop \"fill\"",
        ),
        (
            "import B from \"./b.design\"\n<Frame><B>x</B></Frame>",
            "d/a.design:2:8: B takes no children",
        ),
        (
            "<Frame><Blob /></Frame>",
            "d/a.design:1:8: unknown element Blob",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(problems(source), vec![expected.to_string()], "{source}");
    }
}

/// An image file is decoded whole while resolving, so a broken one is a
/// problem at its `src` — not a blank box — wherever the design is shown.
#[test]
fn an_image_that_does_not_decode_is_a_problem() {
    let mut broken = b"\x89PNG\r\n\x1a\n".to_vec();
    broken.extend(b"not a PNG past its signature");
    let shirt = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/designs/checkout/shirt.jpg"
    ))
    .unwrap();
    let truncated = shirt[..shirt.len() / 2].to_vec();
    for (name, data, expected) in [
        (
            "broken.png",
            broken,
            "d/a.design:2:8: src: ./broken.png is a PNG that doesn't decode: ",
        ),
        (
            "shirt.jpg",
            truncated,
            "d/a.design:2:8: src: ./shirt.jpg is a JPEG that doesn't decode: ",
        ),
    ] {
        let source =
            format!("<Frame>\n<Image src=\"./{name}\" width={{50}} height={{50}} />\n</Frame>");
        let fs = Mem([
            ("d/a.design".to_string(), source.into_bytes()),
            (format!("d/{name}"), data),
        ]
        .into());
        let found: Vec<String> = resolve(&fs, "d/a.design")
            .unwrap_err()
            .iter()
            .map(|p| p.to_string())
            .collect();
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].starts_with(expected), "{found:?}");
    }
    // The whole file decodes: its pixels go with the scene.
    let fs = Mem([
        (
            "a.design".to_string(),
            br#"<Frame><Image src="./s.jpg" /></Frame>"#.to_vec(),
        ),
        ("s.jpg".to_string(), shirt),
    ]
    .into());
    let scene = resolve(&fs, "a.design").unwrap();
    let image = scene.root.children[0].image.as_ref().unwrap();
    let file = &scene.images[&image.id];
    assert_eq!((file.width, file.height), (160, 160));
    assert_eq!((file.pixels_width, file.pixels_height), (160, 160));
}

/// A file larger than [`MAX_IMAGE_SIDE`] is drawn from its pixels
/// resampled to fit it, keeping its aspect; an `Image` still hugs the
/// file's own size.
#[test]
fn an_interaction_is_relative_to_the_file_that_wrote_it() {
    let fs = files(&[
        (
            "app/a.design",
            "import B from \"../lib/b.design\"\n<Frame><B /><B onClick=\"overlay ./c.design\" /></Frame>",
        ),
        ("app/c.design", "<Frame />"),
        (
            "lib/b.design",
            "<Frame onClick=\"navigate ./next.design\" />",
        ),
        ("lib/next.design", "<Frame />"),
    ]);
    let root = resolve(&fs, "app/a.design").unwrap().root;
    assert_eq!(
        root.children[0].on_click.as_deref(),
        Some("navigate lib/next.design")
    );
    assert_eq!(
        root.children[1].on_click.as_deref(),
        Some("overlay app/c.design")
    );
}

#[test]
fn an_interaction_to_nowhere_is_a_problem() {
    for (click, expected) in [
        (
            "navigate ./nope.design",
            "d/a.design:1:8: onClick: navigate ./nope.design — no such file",
        ),
        (
            "overlay ../../x.design",
            "d/a.design:1:8: onClick: overlay ../../x.design — outside the served folder",
        ),
    ] {
        assert_eq!(
            problems(&format!("<Frame onClick=\"{click}\" />")),
            vec![expected.to_string()],
            "{click}"
        );
    }
}

#[test]
fn a_valid_design_has_no_problems() {
    assert_eq!(
        problems(
            r##"import B from "./b.design"
<Frame fill="linear-gradient(90deg, #ffffff 0%, #000000ff 100%)">
  <B label="Go" x={4} y={5} width={80} height="hug" name="Go" onClick="back" />
  <Vector d="M0 0L24 24" viewBox="0 0 24 24" stroke="#000" />
  <Frame radius={8} clip><Image src="./photo.png" radius={[1, 2, 3, 4]} /></Frame>
</Frame>"##
        ),
        Vec::<String>::new()
    );
}

#[test]
fn attributes_take_their_defaults() {
    let fs = files(&[("a.design", "<Frame><Text>a</Text><Rect /></Frame>")]);
    let root = resolve(&fs, "a.design").unwrap().root;
    assert_eq!((root.width, root.height), (Size::Hug, Size::Hug));
    assert_eq!(
        (root.layout, root.gap, root.padding),
        (Layout::None, 0.0, [0.0; 4])
    );
    assert_eq!((root.align, root.justify), (Align::Start, Align::Start));
    assert_eq!(
        (root.opacity, root.fill.clone(), root.clip),
        (1.0, None, false)
    );
    let text = root.children[0].text.clone().unwrap();
    assert_eq!(
        text,
        TextStyle {
            content: "a".into(),
            ..default_text()
        }
    );
    assert_eq!(
        (text.size, text.weight, text.line_height, text.align),
        (14.0, 400, 1.4, TextAlign::Left)
    );
    assert_eq!(root.children[1].stroke_width, 1.0);
    assert_eq!(root.children[1].radius, [0.0; 4]);
}

#[test]
fn values_parse_into_their_types() {
    let fs = Mem(
        [
            (
                "a.design".to_string(),
                br##"<Frame padding={[1, 2]} radius={[1, 2, 3, 4]} fill="linear-gradient(135deg, #f00 0%, #0000ff80 100%)">
  <Frame padding={[1, 2, 3, 4]} fill="#abcd" />
  <Vector d="M2 4L10 20" />
  <Image src="./p.png" />
</Frame>"##
                    .to_vec(),
            ),
            ("p.png".to_string(), png()),
        ]
        .into(),
    );
    let scene = resolve(&fs, "a.design").unwrap();
    let root = &scene.root;
    assert_eq!(root.padding, [1.0, 2.0, 1.0, 2.0]);
    assert_eq!(root.radius, [1.0, 2.0, 3.0, 4.0]);
    let c = |r, g, b, a| Color::fixed(Rgba { r, g, b, a });
    assert_eq!(
        root.fill,
        Some(Fill::Linear {
            angle: 135.0,
            stops: vec![(c(255, 0, 0, 255), 0.0), (c(0, 0, 255, 0x80), 1.0)]
        })
    );
    assert_eq!(root.children[0].padding, [1.0, 2.0, 3.0, 4.0]);
    assert_eq!(
        root.children[0].fill,
        Some(Fill::Solid(c(0xaa, 0xbb, 0xcc, 0xdd)))
    );
    // A vector with no view box is drawn from its path's own bounds.
    assert_eq!(
        root.children[1].vector.as_ref().unwrap().view_box,
        [2.0, 4.0, 8.0, 16.0]
    );
    assert_eq!(root.children[2].image.as_ref().unwrap().src, "p.png");
    assert_eq!(read(&fs, "a.design"), ["a.design", "p.png"]);
}

#[test]
fn paths_stay_in_the_served_folder() {
    assert_eq!(
        join("a/b.design", "./c.design").as_deref(),
        Some("a/c.design")
    );
    assert_eq!(
        join("a/b.design", "../c.design").as_deref(),
        Some("c.design")
    );
    assert_eq!(join("a/b.design", "../../c.design"), None);
    assert_eq!(
        join("/a/b.design", "x/../y.png").as_deref(),
        Some("a/y.png")
    );
}

#[test]
fn an_interaction_leads_only_to_a_design() {
    for (click, expected) in [
        (
            "navigate ./notes.txt",
            "d/a.design:1:8: onClick: navigate ./notes.txt — a design is a .design file",
        ),
        (
            "overlay ./photo.png",
            "d/a.design:1:8: onClick: overlay ./photo.png — a design is a .design file",
        ),
    ] {
        assert_eq!(
            problems(&format!("<Frame onClick=\"{click}\" />")),
            vec![expected.to_string()],
            "{click}"
        );
    }
    assert!(problems(r#"<Frame onClick="navigate ./b.design" />"#).is_empty());
}

#[test]
fn sizes_and_spacing_are_never_negative() {
    for (attr, element) in [
        ("gap={-4}", "Frame"),
        ("padding={-1}", "Frame"),
        ("padding={[4, -2]}", "Frame"),
        ("radius={-3}", "Rect"),
        ("radius={[1, 2, -3, 4]}", "Rect"),
        ("strokeWidth={-1}", "Rect"),
        ("size={-12}", "Text"),
        ("lineHeight={-1}", "Text"),
    ] {
        let found = problems(&format!("<Frame><{element} {attr} /></Frame>"));
        assert_eq!(found.len(), 1, "{attr}: {found:?}");
        assert!(found[0].contains("of 0 or more"), "{attr}: {found:?}");
    }
}

#[test]
fn an_import_name_is_unique_and_not_an_element() {
    let found = problems(
        r#"import B from "./b.design"
import B from "./b.design"
<Frame />"#,
    );
    assert_eq!(found, ["d/a.design:2:1: import B — B is already imported"]);
    let found = problems(
        r#"import Frame from "./b.design"
<Frame />"#,
    );
    assert_eq!(
        found,
        ["d/a.design:1:1: import Frame — Frame is a built-in element"]
    );
}

#[test]
fn justify_names_all_it_takes() {
    assert_eq!(
        problems(r#"<Frame justify="middle" />"#),
        ["d/a.design:1:8: justify: expected \"start\", \"center\", \"end\" or \"space-between\""]
    );
    assert_eq!(
        problems(r#"<Frame align="middle" />"#),
        ["d/a.design:1:8: align: expected \"start\", \"center\" or \"end\""]
    );
}

#[test]
fn values_that_would_be_ignored_are_problems() {
    // x and y place a child only in a frame with no layout.
    let found = problems(r#"<Frame layout="row"><Rect width={10} height={10} x={5} /></Frame>"#);
    assert!(
        found
            .iter()
            .any(|p| p.contains("x: places a child only in a frame with layout=\"none\"")),
        "{found:?}"
    );
    // The same on an instance placed in a row.
    let found = problems(
        r#"import B from "./b.design"
<Frame layout="row"><B y={3} /></Frame>"#,
    );
    assert!(
        found.iter().any(|p| p.contains("y: places a child only")),
        "{found:?}"
    );
    // Negative sizes.
    let found = problems(r#"<Frame width={-1} height={-2} />"#);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(
        found[0].contains("width: expected pixels of 0 or more"),
        "{found:?}"
    );
    // An attribute given twice.
    let found = problems(r##"<Frame fill="#ff0000" fill="#00ff00" />"##);
    assert!(
        found.iter().any(|p| p.contains("fill is given twice")),
        "{found:?}"
    );
    let found = problems(
        r#"import B from "./b.design"
<Frame><B label="a" label="b" /></Frame>"#,
    );
    assert!(
        found.iter().any(|p| p.contains("label is given twice")),
        "{found:?}"
    );
    // An import of something that isn't a design says so.
    let found = problems(
        r#"import P from "./photo.png"
<Frame />"#,
    );
    assert_eq!(
        found,
        ["d/a.design:1:1: import ./photo.png — a design is a .design file"]
    );
    // x and y in a frame with no layout are fine.
    assert!(problems(r#"<Frame><Rect width={1} height={1} x={5} y={5} /></Frame>"#).is_empty());
}

const THEMES: &str = r##"kind: themes
light:
  surface: "#ffffff"
  text: "#000"
dark:
  surface: "#0f172a"
  text: "#f1f5f9"
"##;

fn rgb(r: u8, g: u8, b: u8) -> Rgba {
    Rgba { r, g, b, a: 255 }
}

#[test]
fn a_theme_color_has_a_value_in_each_theme() {
    let fs = files(&[
        ("themes.yml", THEMES),
        (
            "a.design",
            r##"<Frame fill={theme.surface} stroke="#ff0000"
   shadow={{ y: 2, color: theme.text }}>
  <Text color={theme.text}>Hi</Text>
  <Rect width={1} height={1} fill="linear-gradient(90deg, theme.surface 0%, #ff0000 100%)" />
</Frame>"##,
        ),
    ]);
    let scene = resolve(&fs, "a.design").unwrap();
    assert!(scene.themed);
    let surface = Color {
        light: rgb(255, 255, 255),
        dark: rgb(0x0f, 0x17, 0x2a),
    };
    let text = Color {
        light: rgb(0, 0, 0),
        dark: rgb(0xf1, 0xf5, 0xf9),
    };
    let root = &scene.root;
    assert_eq!(root.fill, Some(Fill::Solid(surface)));
    // A # color is the same in both.
    assert_eq!(root.stroke, Some(Color::fixed(rgb(255, 0, 0))));
    assert_eq!(root.shadow.as_ref().unwrap().color, text);
    assert_eq!(root.children[0].text.as_ref().unwrap().color, text);
    assert_eq!(
        root.children[1].fill,
        Some(Fill::Linear {
            angle: 90.0,
            stops: vec![(surface, 0.0), (Color::fixed(rgb(255, 0, 0)), 1.0)]
        })
    );
    assert_eq!(read(&fs, "a.design"), ["a.design", "themes.yml"]);
    // A design with no theme color isn't themed, and reads no themes.
    let fs = files(&[("themes.yml", THEMES), ("b.design", "<Frame />")]);
    let plain = resolve(&fs, "b.design").unwrap();
    assert!(!plain.themed);
    assert_eq!(read(&fs, "b.design"), ["b.design"]);
}

/// Files that record every path asked for.
struct Asked(HashMap<String, String>, std::cell::RefCell<Vec<String>>);

impl Files for Asked {
    fn bytes(&self, path: &str) -> Option<Vec<u8>> {
        self.1.borrow_mut().push(path.to_string());
        self.0.bytes(path)
    }
    fn list(&self, path: &str) -> Option<Vec<String>> {
        self.0.list(path)
    }
}

#[test]
fn the_nearest_themes_override_those_above_name_by_name() {
    let fs = Asked(
        files(&[
            ("themes.yml", THEMES),
            (
                "app/themes.yml",
                "kind: themes\nlight:\n  text: \"#111111\"\ndark: {}\n",
            ),
            // Another tool's file: passed over.
            ("app/screens/themes.yml", "kind: tokens\nlight: 1\n"),
            (
                "app/screens/a.design",
                "<Frame fill={theme.surface}><Text color={theme.text}>Hi</Text></Frame>",
            ),
        ]),
        Default::default(),
    );
    let scene = resolve(&fs, "app/screens/a.design").unwrap();
    assert_eq!(
        scene.root.fill,
        Some(Fill::Solid(Color {
            light: rgb(255, 255, 255),
            dark: rgb(0x0f, 0x17, 0x2a)
        }))
    );
    // Light's text from app/, dark's from the top.
    assert_eq!(
        scene.root.children[0].text.as_ref().unwrap().color,
        Color {
            light: rgb(0x11, 0x11, 0x11),
            dark: rgb(0xf1, 0xf5, 0xf9)
        }
    );
    // Every candidate is asked for once, the missing and the passed-over
    // ones too, so writing any of them counts as a change.
    let mut asked = fs.1.borrow().clone();
    asked.retain(|p| p.ends_with("themes.yml"));
    assert_eq!(
        asked,
        vec!["app/screens/themes.yml", "app/themes.yml", "themes.yml"]
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_theme_color_belongs_to_the_file_that_wrote_it() {
    let dark_red = "kind: themes\nlight:\n  accent: \"#ff0000\"\ndark:\n  accent: \"#880000\"\n";
    let blue = "kind: themes\nlight:\n  accent: \"#0000ff\"\ndark:\n  accent: \"#000088\"\n";
    let fs = files(&[
        ("kit/themes.yml", dark_red),
        (
            "kit/button.design",
            "<Frame props={{ fill: theme.accent }} fill={fill} />",
        ),
        ("app/themes.yml", blue),
        (
            "app/screen.design",
            "import Button from \"../kit/button.design\"\n<Frame layout=\"row\">\n  <Button />\n  <Button fill={theme.accent} />\n</Frame>",
        ),
    ]);
    let scene = resolve(&fs, "app/screen.design").unwrap();
    let fill = |n: usize| scene.root.children[n].fill.clone();
    // The default is the kit's red; the color the screen passes, its blue.
    assert_eq!(
        fill(0),
        Some(Fill::Solid(Color {
            light: rgb(255, 0, 0),
            dark: rgb(0x88, 0, 0)
        }))
    );
    assert_eq!(
        fill(1),
        Some(Fill::Solid(Color {
            light: rgb(0, 0, 255),
            dark: rgb(0, 0, 0x88)
        }))
    );
    assert!(scene.themed);
}

#[test]
fn a_theme_color_that_isnt_there_is_a_problem_at_the_reference() {
    let one_sided =
        "kind: themes\nlight:\n  surface: \"#fff\"\n  edge: \"#000\"\ndark:\n  surface: \"#000\"\n";
    let design = r##"<Frame fill={theme.srface}>
  <Rect width={1} height={1} fill={theme.edge} />
  <Rect width={1} height={1} fill="linear-gradient(90deg, #000 0%, theme.nope 100%)" />
  <Text>{theme.surface}</Text>
</Frame>"##;
    let fs = files(&[("themes.yml", one_sided), ("a.design", design)]);
    assert_eq!(
        problems_in(&fs, "a.design"),
        vec![
            "a.design:1:14: theme.srface: the themes have no srface; they have edge, surface",
            "a.design:2:36: theme.edge: only the light theme has edge; a name needs a color in both",
            "a.design:3:68: theme.nope: the themes have no nope; they have edge, surface",
            "a.design:4:10: theme.surface is a color, not text: give it to color={theme.surface}",
        ]
    );
    // With no themes file above the design at all.
    let fs = files(&[("a/a.design", "<Frame fill={theme.surface} />")]);
    assert_eq!(
        problems_in(&fs, "a/a.design"),
        vec![
            "a/a.design:1:14: theme.surface: no themes.yml with kind: themes in this design's folder or above it"
        ]
    );
}

#[test]
fn a_malformed_themes_file_is_a_problem_at_its_line() {
    let broken = r##"kind: themes
# the light one
light:
  surface: "#ffff"
  text: "#gg0000"
  edge: #000
  9lives: "#000"
dark: "#000"
colour: red
"##;
    let fs = files(&[
        ("themes.yml", broken),
        ("a.design", "<Frame fill={theme.surface} />"),
    ]);
    assert_eq!(
        problems_in(&fs, "a.design"),
        vec![
            "themes.yml:5:3: light.text: #gg0000 is not a color",
            "themes.yml:6:3: light.edge: no color: write it in quotes, \"#rrggbb\", as YAML reads # as a comment",
            "themes.yml:7:3: light.9lives: a name holds letters, digits, _ and -, and starts with a letter",
            "themes.yml:8:1: dark: expected a map of names to colors, like surface: \"#ffffff\"",
            "themes.yml:9:1: colour: a themes file has kind, fonts, light and dark only",
            "a.design:1:14: theme.surface: only the light theme has surface; a name needs a color in both",
        ]
    );
    let fs = files(&[
        ("themes.yml", "kind: themes\nlight: {}\n"),
        ("a.design", "<Frame fill={theme.surface} />"),
    ]);
    assert_eq!(
        problems_in(&fs, "a.design")[0],
        "themes.yml:1:1: a themes file has a light and a dark theme; this one has no dark"
    );
    let fs = files(&[
        ("themes.yml", "kind: themes\nlight: [\n"),
        ("a.design", "<Frame fill={theme.surface} />"),
    ]);
    assert!(
        problems_in(&fs, "a.design")[0]
            .starts_with("themes.yml:3:1: this themes file isn't valid YAML: "),
        "{:?}",
        problems_in(&fs, "a.design")
    );
    // A file that doesn't say it is a themes file is someone else's.
    let fs = files(&[("themes.yml", "light: [\n"), ("a.design", "<Frame />")]);
    assert!(resolve(&fs, "a.design").is_ok());
}

#[test]
fn theme_is_no_props_or_imports_name() {
    let fs = files(&[
        ("themes.yml", THEMES),
        ("b.design", "<Frame />"),
        (
            "a.design",
            "import theme from \"./b.design\"\n<Frame props={{ theme: \"#fff\" }} fill={theme} />",
        ),
    ]);
    assert_eq!(
        problems_in(&fs, "a.design"),
        vec![
            "a.design:1:1: import theme — theme is the name of the design's themes",
            "a.design:2:8: props: theme is the name of the design's themes, not a prop",
            "a.design:2:34: fill: no prop \"theme\"",
        ]
    );
    // In quotes it is a string, not a color.
    let fs = files(&[
        ("themes.yml", THEMES),
        ("a.design", "<Frame fill=\"theme.surface\" />"),
    ]);
    assert_eq!(
        problems_in(&fs, "a.design"),
        vec![
            "a.design:1:8: fill: theme.surface is a theme color: write it in braces, {theme.surface}, not in quotes"
        ]
    );
}

fn figtree() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/designs/checkout/fonts/Figtree/Figtree-Variable.ttf"
    ))
    .unwrap()
}

fn inter(face: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/fonts/Inter-{face}.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn mem(list: &[(&str, Vec<u8>)]) -> Mem {
    Mem(list
        .iter()
        .map(|(p, b)| (p.to_string(), b.clone()))
        .collect())
}

fn fonts_of(node: &Node, out: &mut Vec<Option<FontId>>) {
    if let Some(t) = &node.text {
        out.push(t.font);
    }
    for c in &node.children {
        fonts_of(c, out);
    }
}

/// A themes file naming `fonts`.
fn named(fonts: &str) -> Vec<u8> {
    format!("kind: themes\nfonts:\n{fonts}light: {{}}\ndark: {{}}\n").into_bytes()
}

/// A text is set in the font its `font` names, through the themes of
/// the file it is written in — a lower `themes.yml` adding and
/// overriding names, inheriting the rest — or in Inter when it names
/// none; one design mixes families, and a folder's licence is passed
/// over.
#[test]
fn a_text_is_set_in_the_font_it_names_or_inter() {
    let fig = FontId::of(&figtree());
    let bold = FontId::of(&inter("Bold"));
    let fs = mem(&[
        (
            "themes.yml",
            named("  figtree: ./fonts/Figtree\n  heading: ./fonts/Figtree\n"),
        ),
        ("fonts/Figtree/Figtree-Variable.ttf", figtree()),
        ("fonts/Figtree/OFL.txt", b"licence".to_vec()),
        ("fonts/Inter/Inter-Bold.ttf", inter("Bold")),
        ("fonts/Inter/Inter-Regular.ttf", inter("Regular")),
        ("app/themes.yml", named("  heading: ../fonts/Inter\n")),
        (
            "app/a.design",
            br#"<Frame><Text font="figtree">a</Text><Text font="heading" weight={700}>b</Text><Text>c</Text></Frame>"#
                .to_vec(),
        ),
    ]);
    let scene = resolve(&fs, "app/a.design").unwrap();
    let mut found = Vec::new();
    fonts_of(&scene.root, &mut found);
    assert_eq!(found, vec![Some(fig), Some(bold), None]);
    assert_eq!(scene.fonts.len(), 2);
    assert!(!scene.themed, "a font is no theme color");
    let plain = files(&[("a.design", "<Frame><Text>a</Text></Frame>")]);
    assert!(resolve(&plain, "a.design").unwrap().fonts.is_empty());
}

/// Weight picks the face: the static file of that weight, or the
/// variable font set to it — and a scene carries only the faces its
/// texts use, each once.
#[test]
fn weight_picks_the_face_and_a_scene_carries_only_those_used() {
    let fs = mem(&[
        ("themes.yml", named("  inter: ./f\n  fig: ./v.ttf\n")),
        ("f/Inter-Regular.ttf", inter("Regular")),
        ("f/Inter-Medium.ttf", inter("Medium")),
        ("f/Inter-SemiBold.ttf", inter("SemiBold")),
        ("f/Inter-Bold.ttf", inter("Bold")),
        ("v.ttf", figtree()),
        (
            "a.design",
            br#"<Frame><Text font="inter" weight={600}>a</Text><Text font="inter" weight={600}>b</Text><Text font="inter">c</Text><Text font="fig" weight={800}>d</Text><Text font="fig" weight={300}>e</Text></Frame>"#
                .to_vec(),
        ),
    ]);
    let scene = resolve(&fs, "a.design").unwrap();
    let mut found = Vec::new();
    fonts_of(&scene.root, &mut found);
    let (semi, regular, fig) = (
        FontId::of(&inter("SemiBold")),
        FontId::of(&inter("Regular")),
        FontId::of(&figtree()),
    );
    assert_eq!(
        found,
        vec![Some(semi), Some(semi), Some(regular), Some(fig), Some(fig)]
    );
    assert_eq!(scene.fonts.len(), 3, "medium and bold aren't used");
}

fn font_problems(list: &[(&str, Vec<u8>)], path: &str) -> Vec<String> {
    match resolve(&mem(list), path) {
        Ok(_) => Vec::new(),
        Err(p) => p.iter().map(|p| p.to_string()).collect(),
    }
}

/// A weight the font lacks is a problem at the `weight`, or at the
/// `Text` when it leaves the weight at 400, listing those it has.
#[test]
fn a_weight_the_font_lacks_is_a_problem_listing_those_it_has() {
    let design = |text: &str| {
        vec![
            ("themes.yml", named("  fig: ./v.ttf\n  bold: ./b\n")),
            ("v.ttf", figtree()),
            ("b/Inter-Bold.ttf", inter("Bold")),
            ("a.design", format!("<Frame>{text}</Frame>").into_bytes()),
        ]
    };
    for (text, expected) in [
        (
            r#"<Text weight={300}>a</Text>"#,
            "a.design:1:14: weight: Inter has no 300; it has 400, 500, 600 and 700",
        ),
        (
            r#"<Text font="fig" weight={200}>a</Text>"#,
            "a.design:1:25: weight: Figtree has no 200; it has 300, 400, 500, 600, 700, 800 and 900",
        ),
        (
            r#"<Text font="bold">a</Text>"#,
            "a.design:1:8: weight: Inter has no 400; it has 700",
        ),
    ] {
        assert_eq!(
            font_problems(&design(text), "a.design"),
            vec![expected],
            "{text}"
        );
    }
}

/// A name the themes don't give is a problem at the `font`, listing the
/// names they do; a font that can't be used is one at its entry in the
/// `themes.yml`, once however many texts use it.
#[test]
fn a_font_that_cant_be_used_is_a_problem_at_its_entry() {
    assert_eq!(
        font_problems(
            &[
                ("themes.yml", named("  fig: ./v.ttf\n  body: ./v.ttf\n")),
                ("v.ttf", figtree()),
                (
                    "a.design",
                    br#"<Frame><Text font="figg">a</Text></Frame>"#.to_vec()
                ),
            ],
            "a.design"
        ),
        vec!["a.design:1:14: font: no font named figg; the themes name body, fig"]
    );
    assert_eq!(
        font_problems(
            &[(
                "a.design",
                br#"<Frame><Text font="fig">a</Text></Frame>"#.to_vec()
            )],
            "a.design"
        ),
        vec![
            "a.design:1:14: font: no font named fig; no themes.yml in this design's folder or above it names fonts"
        ]
    );
    assert_eq!(
        font_problems(
            &[(
                "a.design",
                br#"<Frame><Text font={3}>a</Text></Frame>"#.to_vec()
            )],
            "a.design"
        ),
        vec!["a.design:1:14: font: expected the name of a font the themes name, like \"figtree\""]
    );
    let mut big = vec![0, 1, 0, 0];
    big.resize(fonts::MAX_FONT_BYTES + 1, 0);
    let mut copy = inter("Regular");
    copy.push(0);
    let list = vec![
        ("d/v.ttf", figtree()),
        ("d/notes.txt", b"hi".to_vec()),
        ("d/web.woff2", b"wOF2rest".to_vec()),
        ("d/web.woff", b"wOFFrest".to_vec()),
        ("d/all.ttc", b"ttcfrest".to_vec()),
        ("d/big.ttf", big),
        ("d/broken.ttf", b"\0\x01\0\0broken".to_vec()),
        ("d/empty/OFL.txt", b"licence".to_vec()),
        ("d/mixed/Figtree.ttf", figtree()),
        ("d/mixed/Inter.ttf", inter("Regular")),
        ("d/twice/a.ttf", inter("Regular")),
        ("d/twice/b.ttf", copy),
        ("d/bad/a.ttf", b"nope".to_vec()),
    ];
    for (path, expected) in [
        ("./nope", "— no such file or folder"),
        ("../../up", "— outside the served folder"),
        (
            "./notes.txt",
            "is not a font: devpage reads .ttf and .otf files",
        ),
        (
            "./web.woff2",
            "is a WOFF2 file, which devpage doesn't read — decompress it to a .ttf or .otf",
        ),
        (
            "./web.woff",
            "is a WOFF file, which devpage doesn't read — decompress it to a .ttf or .otf",
        ),
        (
            "./all.ttc",
            "is a collection of fonts; name one font's own .ttf or .otf file",
        ),
        (
            "./big.ttf",
            "is 4.0 MB; a font file is at most 4 MB — subset it to the characters the design shows",
        ),
        (
            "./broken.ttf",
            "is a .ttf or .otf file that doesn't read as a font",
        ),
        ("./empty", "holds no .ttf or .otf file"),
        (
            "./mixed",
            "holds more than one family: Figtree (Figtree.ttf), Inter (Inter.ttf)",
        ),
        ("./twice", "holds two files of weight 400: a.ttf and b.ttf"),
        (
            "./bad",
            "holds a.ttf, which is not a font: devpage reads .ttf and .otf files",
        ),
    ] {
        let mut files = list.clone();
        files.push(("d/themes.yml", named(&format!("  x: {path}\n"))));
        files.push((
            "d/a.design",
            br#"<Frame><Text font="x">a</Text><Text font="x">b</Text></Frame>"#.to_vec(),
        ));
        assert_eq!(
            font_problems(&files, "d/a.design"),
            vec![format!("d/themes.yml:3:3: fonts.x: {path} {expected}")]
        );
    }
}

/// A themes file's `fonts` is a map of names — lowercase letters, digits
/// and `-` — to paths; anything else is a problem at its line, and
/// `font` is no key of a themes file.
#[test]
fn a_themes_files_fonts_are_names_to_paths() {
    let problems = font_problems(
        &[
            (
                "themes.yml",
                b"kind: themes\nfonts:\n  Figtree: ./v.ttf\n  my_font: ./v.ttf\n  ok: [1]\nfont: ./v.ttf\nlight: {}\ndark: {}\n"
                    .to_vec(),
            ),
            ("a.design", b"<Frame><Text>a</Text></Frame>".to_vec()),
        ],
        "a.design",
    );
    assert_eq!(
        problems,
        Vec::<String>::new(),
        "read only for a text that names a font"
    );
    let problems = font_problems(
        &[
            (
                "themes.yml",
                b"kind: themes\nfonts:\n  Figtree: ./v.ttf\n  my_font: ./v.ttf\n  ok: [1]\nfont: ./v.ttf\nlight: {}\ndark: {}\n"
                    .to_vec(),
            ),
            ("a.design", br#"<Frame><Text font="ok">a</Text></Frame>"#.to_vec()),
        ],
        "a.design",
    );
    assert_eq!(
        problems,
        vec![
            "themes.yml:3:3: fonts.Figtree: a font's name holds lowercase letters, digits and - only",
            "themes.yml:4:3: fonts.my_font: a font's name holds lowercase letters, digits and - only",
            "themes.yml:5:3: fonts.ok: expected a path to a font file or a folder of one family's, like ./fonts/Figtree",
            "themes.yml:6:1: font: a themes file has kind, fonts, light and dark only",
            "a.design:1:14: font: no font named ok; no themes.yml in this design's folder or above it names fonts",
        ]
    );
}

/// A value given as a prop — an image's `src`, a text's `font`, an
/// interaction — is read where it was written: a path relative to that
/// file, a font's name through its themes; its problem is there too.
#[test]
fn a_value_given_as_a_prop_is_read_where_it_was_written() {
    let list = |caller: &str| {
        vec![
            (
                "app/a.design",
                format!("import Card from \"../lib/card.design\"\n<Frame>{caller}</Frame>")
                    .into_bytes(),
            ),
            ("app/themes.yml", named("  mine: ./v.ttf\n")),
            ("app/me.png", png()),
            ("app/v.ttf", figtree()),
            ("app/next.design", b"<Frame />".to_vec()),
            ("lib/themes.yml", named("  own: ./own.ttf\n")),
            (
                "lib/card.design",
                br#"<Frame props={{ src: "./own.png", font: "own", go: "back" }} onClick={go}><Image src={src} /><Text font={font}>a</Text></Frame>"#
                    .to_vec(),
            ),
            ("lib/own.png", png()),
            ("lib/own.ttf", inter("Regular")),
        ]
    };
    let scene = resolve(
        &mem(&list(
            r#"<Card src="./me.png" font="mine" go="navigate ./next.design" />"#,
        )),
        "app/a.design",
    )
    .unwrap();
    let card = &scene.root.children[0];
    assert_eq!(card.children[0].image.as_ref().unwrap().src, "app/me.png");
    assert_eq!(
        card.children[1].text.as_ref().unwrap().font,
        Some(FontId::of(&figtree()))
    );
    assert_eq!(card.on_click.as_deref(), Some("navigate app/next.design"));
    // The defaults are the design's own file's.
    let scene = resolve(&mem(&list("<Card />")), "app/a.design").unwrap();
    let card = &scene.root.children[0];
    assert_eq!(card.children[0].image.as_ref().unwrap().src, "lib/own.png");
    assert_eq!(
        card.children[1].text.as_ref().unwrap().font,
        Some(FontId::of(&inter("Regular")))
    );
    // A bad value is a problem where it was written.
    assert_eq!(
        font_problems(
            &list(r#"<Card src="./no.png" font="own" go="overlay ./no.design" />"#),
            "app/a.design"
        ),
        vec![
            "app/a.design:2:43: onClick: overlay ./no.design — no such file",
            "app/a.design:2:18: src: ./no.png — no such file",
            "app/a.design:2:34: font: no font named own; the themes name mine",
        ]
    );
}
