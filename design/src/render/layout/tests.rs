use std::collections::HashMap;

use crate::render::testing::*;
use crate::resolve::{self, Scene, Theme};

use super::*;

#[test]
fn auto_layout_places_children() {
    let s = scene(
        r#"<Frame width={200} height={100} layout="row" gap={10} padding={20}>
  <Rect width={30} height={30} />
  <Rect width="fill" height="fill" />
</Frame>"#,
    );
    let frames = Renderer::new().layout(&s);
    assert_eq!(
        frames[1],
        Frame {
            x: 20.0,
            y: 20.0,
            width: 30.0,
            height: 30.0
        }
    );
    assert_eq!(
        frames[2],
        Frame {
            x: 60.0,
            y: 20.0,
            width: 120.0,
            height: 60.0
        }
    );
}

#[test]
fn text_hugs_its_content() {
    let s = scene(r#"<Frame layout="column" padding={10}><Text size={20}>Hello</Text></Frame>"#);
    let frames = Renderer::new().layout(&s);
    assert!(
        frames[1].width > 40.0 && frames[1].width < 60.0,
        "{:?}",
        frames[1]
    );
    assert_eq!(frames[1].height, 28.0);
    assert_eq!(frames[0].height, 48.0);
}

/// The repository's files a design can set text in, and one design.
struct Fonts(HashMap<String, Vec<u8>>);

impl crate::Files for Fonts {
    fn bytes(&self, path: &str) -> Option<Vec<u8>> {
        self.0.get(path).cloned()
    }
    fn list(&self, path: &str) -> Option<Vec<String>> {
        resolve::listed(self.0.keys(), path)
    }
}

fn with_fonts(source: &str) -> Scene {
    let root = env!("CARGO_MANIFEST_DIR");
    let read = |p: String| std::fs::read(p).unwrap();
    let files = Fonts(
        [
            (
                "v.ttf".to_string(),
                read(format!(
                    "{root}/../docs/designs/checkout/fonts/Figtree/Figtree-Variable.ttf"
                )),
            ),
            (
                "f/Inter-Regular.ttf".to_string(),
                read(format!("{root}/fonts/Inter-Regular.ttf")),
            ),
            (
                "themes.yml".to_string(),
                b"kind: themes\nfonts:\n  fig: ./v.ttf\n  inter: ./f\nlight: {}\ndark: {}\n"
                    .to_vec(),
            ),
            ("a.design".to_string(), source.as_bytes().to_vec()),
        ]
        .into(),
    );
    resolve::resolve(&files, "a.design").unwrap()
}

/// A text is measured in its font, and a variable font at its weight:
/// its glyph runs carry the point on the weight axis, which a static
/// font's don't.
#[test]
fn a_text_is_laid_out_in_its_font_at_its_weight() {
    let line = |font: &str, weight: u16| {
        let s = with_fonts(&format!(
            r#"<Frame><Text size={{40}} weight={{{weight}}}{font}>Mmmmm wwwww</Text></Frame>"#
        ));
        let mut r = Renderer::new();
        let width = r.layout(&s)[1].width;
        let d = r.drawing(&s, Theme::Light, None).unwrap();
        let coords: Vec<Vec<i16>> = d
            .list
            .chunks()
            .flat_map(|(_, c)| c.commands.iter().map(|c| &c.draw))
            .filter_map(|d| match d {
                Draw::Glyphs(_, _, _, coords, _) => Some(coords.clone()),
                _ => None,
            })
            .collect();
        (width, coords)
    };
    let (inter, coords) = line("", 400);
    assert!(coords.iter().all(|c| c.is_empty()));
    let (same, _) = line(r#" font="inter""#, 400);
    assert_eq!(
        same, inter,
        "Inter from a folder measures as the embedded one"
    );
    let (light, light_coords) = line(r#" font="fig""#, 400);
    let (heavy, heavy_coords) = line(r#" font="fig""#, 900);
    assert_ne!(light, inter);
    assert!(heavy > light, "{heavy} ≤ {light}");
    assert!(!heavy_coords.is_empty() && heavy_coords.iter().all(|c| !c.is_empty()));
    assert_ne!(light_coords, heavy_coords);
}

/// A renderer that laid out scenes in other fonts lays a scene out as a
/// fresh one does, and holds no more than one scene's font files.
#[test]
fn a_renderer_lays_out_as_a_fresh_one_whatever_it_drew_before() {
    let a = with_fonts(
        r#"<Frame width={120} layout="column"><Text width="fill" font="fig" weight={700}>Washed linen with a relaxed fit</Text></Frame>"#,
    );
    let b = with_fonts(
        r#"<Frame width={120} layout="column"><Text width="fill" font="inter">Washed linen with a relaxed fit</Text></Frame>"#,
    );
    let mut used = Renderer::new();
    for s in [&a, &b, &a, &b] {
        assert_eq!(used.layout(s), Renderer::new().layout(s));
        assert_eq!(used.registered.as_ref().unwrap().len(), 1);
    }
    let inter = scene(
        r#"<Frame width={120} layout="column"><Text width="fill">Washed linen with a relaxed fit</Text></Frame>"#,
    );
    assert_eq!(used.layout(&inter), Renderer::new().layout(&inter));
}

fn frames(source: &str) -> Vec<Frame> {
    Renderer::new().layout(&scene(source))
}

#[test]
fn hug_takes_the_content_and_padding() {
    let f = frames(
        r#"<Frame layout="row" gap={4} padding={[2, 6]}>
  <Rect width={10} height={20} /><Rect width={10} height={8} />
</Frame>"#,
    );
    assert_eq!(f[0], at(0.0, 0.0, 36.0, 24.0));
    assert_eq!(f[1], at(6.0, 2.0, 10.0, 20.0));
    assert_eq!(f[2], at(20.0, 2.0, 10.0, 8.0));
}

#[test]
fn fill_shares_the_free_space_and_stretches_across() {
    let f = frames(
        r#"<Frame width={100} height={50} layout="column" padding={[5, 10, 15, 20]}>
  <Rect height={10} width="fill" /><Rect height="fill" width={10} /><Rect height="fill" width={10} />
</Frame>"#,
    );
    assert_eq!(f[1], at(20.0, 5.0, 70.0, 10.0));
    assert_eq!(f[2], at(20.0, 15.0, 10.0, 10.0));
    assert_eq!(f[3], at(20.0, 25.0, 10.0, 10.0));
}

#[test]
fn align_and_justify_place_children() {
    let row = |align: &str, justify: &str| {
        frames(&format!(
            r#"<Frame width={{100}} height={{40}} layout="row" align="{align}" justify="{justify}">
  <Rect width={{10}} height={{10}} /><Rect width={{20}} height={{20}} />
</Frame>"#
        ))
    };
    let f = row("center", "center");
    assert_eq!((f[1].x, f[1].y, f[2].x, f[2].y), (35.0, 15.0, 45.0, 10.0));
    let f = row("end", "end");
    assert_eq!((f[1].x, f[1].y, f[2].x, f[2].y), (70.0, 30.0, 80.0, 20.0));
    let f = row("start", "space-between");
    assert_eq!((f[1].x, f[1].y, f[2].x, f[2].y), (0.0, 0.0, 80.0, 0.0));
}

#[test]
fn no_layout_places_children_at_their_position() {
    let f = frames(
        r#"<Frame width={100} height={100}>
  <Rect x={10} y={-5} width={20} height={20} /><Rect x={50} y={60} width={5} height={5} />
</Frame>"#,
    );
    assert_eq!(f[1], at(10.0, -5.0, 20.0, 20.0));
    assert_eq!(f[2], at(50.0, 60.0, 5.0, 5.0));
}

#[test]
fn a_vector_hugs_its_view_box_and_keeps_its_aspect() {
    let f = frames(
        r#"<Frame layout="row">
  <Vector d="M0 0L24 12" viewBox="0 0 24 12" /><Vector d="M0 0L24 12" viewBox="0 0 24 12" height={6} />
</Frame>"#,
    );
    assert_eq!(f[1], at(0.0, 0.0, 24.0, 12.0));
    assert_eq!(f[2], at(24.0, 0.0, 12.0, 6.0));
}

#[test]
fn centered_text_sits_in_the_middle_of_its_width() {
    let left = frames(r#"<Frame><Text width={300}>Hi</Text></Frame>"#);
    let s = scene(
        r##"<Frame width={300} height={40} fill="#ffffff"><Text width={300} textAlign="center" color="#000000">Hi</Text></Frame>"##,
    );
    let p = Renderer::new().paint(&s, Theme::Light, 1.0);
    let dark = |x: u16| (0..20).any(|y| p.sample(x, y).r < 128);
    assert_eq!(left[1].width, 300.0);
    assert!((140..160).any(dark), "ink near the middle");
    assert!(
        !(0..120).any(dark) && !(180..300).any(dark),
        "none near the edges"
    );
}

#[test]
fn a_hugging_frame_with_no_layout_takes_its_childrens_extent() {
    let s = scene(
        r##"<Frame fill="#ff0000"><Rect x={10} y={5} width={100} height={50} fill="#0000ff" /></Frame>"##,
    );
    let mut r = Renderer::new();
    let frames = r.layout(&s);
    assert_eq!((frames[0].width, frames[0].height), (110.0, 55.0));
    // Nested: the inner frame settles first, then the outer one.
    let s = scene(
        r##"<Frame><Frame x={20} y={0}><Rect x={0} y={30} width={10} height={10} /></Frame></Frame>"##,
    );
    let frames = r.layout(&s);
    assert_eq!((frames[0].width, frames[0].height), (30.0, 40.0));
    assert_eq!((frames[1].width, frames[1].height), (10.0, 40.0));
    // A fixed side stays fixed; only the hugging one follows the children.
    let s = scene(r##"<Frame width={300}><Rect width={10} height={70} /></Frame>"##);
    let frames = r.layout(&s);
    assert_eq!((frames[0].width, frames[0].height), (300.0, 70.0));
}

#[test]
fn a_root_that_fills_on_its_own_hugs() {
    let mut r = Renderer::new();
    for root in [
        r##"<Frame width="fill" height="fill" fill="#ff0000"><Rect width={100} height={50} /></Frame>"##,
        r##"<Frame width="fill" layout="row" fill="#ff0000"><Rect width={100} height={50} /></Frame>"##,
    ] {
        let d = r.drawing(&scene(root), Theme::Light, None).unwrap();
        assert_eq!((d.area().width, d.area().height), (100.0, 50.0), "{root}");
    }
}

#[test]
fn a_drawing_is_laid_out_as_the_layout_lays_it_out() {
    let mut r = Renderer::new();
    let s = scene(r#"<Frame layout="row" padding={4}><Rect width={10} height={10} /></Frame>"#);
    let d = r.drawing(&s, Theme::Light, None).unwrap();
    assert_eq!(d.frames(), r.layout(&s));
    assert_eq!(d.list.chunks().count(), 2);
}

/// Every box as a tree laid out by taffy with its own rounding pass on —
/// each location rounded where taffy's pass rounds it, summed down the
/// tree — which is what the engine's rounding of only the boxes it reads
/// must give, box for box.
/// Each box of `ours` the box taffy gives it, rounded — or, where an edge
/// differs, one whose unrounded edge in taffy's tree lies within a
/// thousandth of a pixel of a rounding boundary: where prefix sums and
/// taffy's own one-by-one sums of sizes that aren't exact in single
/// precision round to either side.
fn agrees(ours: &[Frame], s: &Scene) -> Result<(), String> {
    let whole = rounded_whole(s);
    if ours.len() != whole.len() {
        return Err(format!("{} boxes, taffy {}", ours.len(), whole.len()));
    }
    let raw = unrounded_whole(s);
    let near = |v: f32| ((v - v.floor()) - 0.5).abs() < 1e-3;
    for (i, ((a, b), r)) in ours.iter().zip(&whole).zip(&raw).enumerate() {
        let edges = [
            (a.x, b.x, r.0),
            (a.y, b.y, r.1),
            (a.x + a.width, b.x + b.width, r.2),
            (a.y + a.height, b.y + b.height, r.3),
        ];
        if edges.iter().any(|&(o, t, raw)| o != t && !near(raw)) {
            let all: Vec<usize> = ours
                .iter()
                .zip(&whole)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .map(|(i, _)| i)
                .collect();
            let last = *all.last().unwrap();
            return Err(format!(
                "box {i}: ours {a:?} taffy {b:?}; differing {all:?}; last ours {:?} taffy {:?}",
                ours[last], whole[last]
            ));
        }
    }
    Ok(())
}

/// Every box's unrounded edges in a taffy tree of the whole scene: left,
/// top, right, bottom.
fn unrounded_whole(s: &Scene) -> Vec<(f32, f32, f32, f32)> {
    let mut r = Renderer::new();
    r.register(&s.fonts);
    let mut tree: TaffyTree<Option<Leaf>> = TaffyTree::new();
    let mut hugs = Vec::new();
    let root = build_plain(&mut tree, &s.root, None, &mut hugs, &s.images);
    r.settle_in(
        &mut Runs::default(),
        &mut tree,
        root,
        Size::MAX_CONTENT,
        &hugs,
    );
    fn go(
        tree: &TaffyTree<Option<Leaf>>,
        id: NodeId,
        x: f32,
        y: f32,
        out: &mut Vec<(f32, f32, f32, f32)>,
    ) {
        let l = tree.unrounded_layout(id);
        let (x, y) = (x + l.location.x, y + l.location.y);
        out.push((x, y, x + l.size.width, y + l.size.height));
        for child in tree.children(id).expect("children") {
            go(tree, child, x, y, out);
        }
    }
    let mut out = Vec::new();
    go(&tree, root, 0.0, 0.0, &mut out);
    out
}

fn rounded_whole(s: &Scene) -> Vec<Frame> {
    let mut r = Renderer::new();
    r.register(&s.fonts);
    let mut tree: TaffyTree<Option<Leaf>> = TaffyTree::new();
    let mut hugs = Vec::new();
    let root = build_plain(&mut tree, &s.root, None, &mut hugs, &s.images);
    r.settle_in(
        &mut Runs::default(),
        &mut tree,
        root,
        Size::MAX_CONTENT,
        &hugs,
    );
    fn go(tree: &TaffyTree<Option<Leaf>>, id: NodeId, x: f32, y: f32, out: &mut Vec<Frame>) {
        let l = tree.layout(id).expect("laid out");
        let (x, y) = (x + l.location.x, y + l.location.y);
        out.push(Frame {
            x,
            y,
            width: l.size.width,
            height: l.size.height,
        });
        for child in tree.children(id).expect("children") {
            go(tree, child, x, y, out);
        }
    }
    let mut out = Vec::new();
    go(&tree, root, 0.0, 0.0, &mut out);
    out
}

/// A box placed absolutely in a frame with no layout, inside auto layout, at
/// fractional places: measured from its own parent — every frame is the
/// containing block of what is placed in it — and rounded as taffy rounds
/// a whole tree.
#[test]
fn boxes_round_as_a_tree_rounded_whole() {
    let s = scene(
        r#"<Frame layout="column" padding={33.3}><Frame width={100} height={100}><Rect x={10.4} y={10.4} width={20.3} height={20.3} /></Frame></Frame>"#,
    );
    assert_eq!(Renderer::new().layout(&s), rounded_whole(&s));
    assert_eq!(
        Renderer::new().layout(&s)[2],
        Frame {
            x: 44.0,
            y: 44.0,
            width: 20.0,
            height: 20.0
        }
    );
}

/// Random trees — auto layout and none nested in each other, boxes placed
/// absolutely inside boxes placed absolutely, fractional sizes, places,
/// padding and gaps — lay out box for box as taffy's whole-tree rounding
/// does, through a fresh layout and through a drawing.
#[test]
fn random_fractional_trees_round_as_a_tree_rounded_whole() {
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self, n: usize) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 33) as usize) % n.max(1)
        }
        /// A number with a fraction in eighths: exact in f32, so taffy's
        /// own sums of them are exact and a run's exact sums meet them.
        fn frac(&mut self, max: usize) -> String {
            let eighths = ["0", "125", "25", "375", "5", "625", "75", "875"];
            format!("{}.{}", self.next(max), eighths[self.next(8)])
        }
    }
    fn frame(rng: &mut Lcg, depth: usize, parent_none: bool) -> String {
        let at = if parent_none {
            format!(" x={{{}}} y={{{}}}", rng.frac(40), rng.frac(40))
        } else {
            String::new()
        };
        let size = match rng.next(3) {
            0 => String::new(),
            _ => format!(" width={{{}}} height={{{}}}", rng.frac(120), rng.frac(120)),
        };
        if depth == 0 || rng.next(4) == 0 {
            return format!(
                "<Rect{at} width={{{}}} height={{{}}} />",
                rng.frac(50),
                rng.frac(50)
            );
        }
        let layout = ["row", "column", "none"][rng.next(3)];
        let children: String = (0..1 + rng.next(3))
            .map(|_| frame(rng, depth - 1, layout == "none"))
            .collect();
        format!(
            "<Frame{at}{size} layout=\"{layout}\" padding={{{}}} gap={{{}}}>{children}</Frame>",
            rng.frac(20),
            rng.frac(10)
        )
    }
    let mut rng = Lcg(11);
    let mut r = Renderer::new();
    let mut laid = 0;
    for case in 0..300 {
        let source = frame(&mut rng, 4, false);
        let files: HashMap<String, String> = [("a.design".to_string(), source.clone())].into();
        let Ok(s) = resolve::resolve(&files, "a.design") else {
            continue;
        };
        let whole = rounded_whole(&s);
        assert_eq!(r.layout(&s), whole, "case {case}: {source}");
        let d = r.drawing(&s, Theme::Light, None).unwrap();
        assert_eq!(d.frames(), whole, "case {case}: {source}");
        laid += 1;
    }
    assert!(laid > 200, "only {laid} of the trees resolved");
}

/// Random runs — rows and columns of every `align` and `justify` but
/// `"space-between"`, children of fixed, hugging and stretching sizes,
/// wrapping text, runs inside runs, at fractional places in eighths — and
/// the frames whose children share their room: laid out box for box as
/// taffy lays the whole tree out.
#[test]
fn random_runs_lay_out_as_taffy_does() {
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self, n: usize) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 33) as usize) % n.max(1)
        }
        fn frac(&mut self, max: usize) -> String {
            let eighths = ["0", "125", "25", "375", "5", "625", "75", "875"];
            format!("{}.{}", self.next(max), eighths[self.next(8)])
        }
    }
    fn size(rng: &mut Lcg, max: usize, flows: bool) -> String {
        match rng.next(if flows { 4 } else { 3 }) {
            0 => "\"hug\"".to_string(),
            3 => "\"fill\"".to_string(),
            _ => format!("{{{}}}", rng.frac(max)),
        }
    }
    fn node(rng: &mut Lcg, depth: usize, parent: &str) -> String {
        let flows = parent != "none";
        let at = if !flows {
            format!(" x={{{}}} y={{{}}}", rng.frac(40), rng.frac(40))
        } else {
            String::new()
        };
        match rng.next(5) {
            0 => {
                let words = [
                    "Ship", "it", "today,", "then", "measure", "what", "it", "cost",
                ];
                let n = 1 + rng.next(8);
                let text: Vec<&str> = (0..n).map(|i| words[(i + rng.next(3)) % 8]).collect();
                let width = if flows && rng.next(2) == 0 {
                    " width=\"fill\"".to_string()
                } else if rng.next(2) == 0 {
                    format!(" width={{{}}}", rng.frac(120))
                } else {
                    String::new()
                };
                return format!(
                    "<Text{at}{width} size={{{}}}>{}</Text>",
                    10 + rng.next(14),
                    text.join(" ")
                );
            }
            1 if depth > 0 => {}
            _ if depth == 0 => {
                return format!(
                    "<Rect{at} width={} height={} />",
                    size(rng, 50, false),
                    size(rng, 50, false)
                );
            }
            _ => {}
        }
        let layout = ["row", "column", "none"][rng.next(3)];
        let align = ["start", "center", "end"][rng.next(3)];
        let justify = ["start", "center", "end", "space-between"][rng.next(4)];
        let children: String = (0..1 + rng.next(4))
            .map(|_| node(rng, depth - 1, layout))
            .collect();
        format!(
            "<Frame{at} width={} height={} layout=\"{layout}\" align=\"{align}\" justify=\"{justify}\" padding={{{}}} gap={{{}}}>{children}</Frame>",
            size(rng, 160, flows),
            size(rng, 160, flows),
            rng.frac(20),
            rng.frac(10)
        )
    }
    let mut rng = Lcg(29);
    let mut r = Renderer::new();
    let mut laid = 0;
    for case in 0..400 {
        let source = node(&mut rng, 4, "row");
        let files: HashMap<String, String> = [("a.design".to_string(), source.clone())].into();
        let Ok(s) = resolve::resolve(&files, "a.design") else {
            continue;
        };
        if let Err(e) = agrees(&r.layout(&s), &s) {
            panic!("case {case}: {e}\n{source}");
        }
        let d = r.drawing(&s, Theme::Light, None).unwrap();
        if let Err(e) = agrees(&d.frames(), &s) {
            panic!("case {case}, drawn: {e}\n{source}");
        }
        laid += 1;
    }
    assert!(laid > 250, "only {laid} of the trees resolved");
}
