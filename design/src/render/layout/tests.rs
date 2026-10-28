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
