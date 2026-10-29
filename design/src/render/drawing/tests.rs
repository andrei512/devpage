use crate::compare::*;
use std::collections::HashMap;

use crate::render::testing::*;
use crate::scene::{self, Theme};

use super::*;

#[test]
fn the_drawing_is_the_root_plus_what_is_painted_outside_it() {
    let mut r = Renderer::new();
    // Paint inside the root: the drawing is exactly the root.
    let s = scene(
        r##"<Frame width={40} height={30} radius={12} fill="#000000"><Text>Hi</Text></Frame>"##,
    );
    let area = r.drawing(&s, Theme::Light, None).unwrap().area();
    assert_eq!(
        area,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 30.0
        }
    );
    assert_eq!(r.paint(&s, Theme::Light, 1.0).width(), 40);
    // A shadow and a child past the edge widen it; the PNG shows them.
    let s = scene(
        r##"<Frame width={40} height={30} fill="#ffffff" shadow={{ y: 4, blur: 8, color: "#000000ff" }}>
  <Rect x={30} y={-10} width={20} height={10} fill="#ff0000" />
</Frame>"##,
    );
    let area = r.drawing(&s, Theme::Light, None).unwrap().area();
    assert_eq!(
        area,
        Frame {
            x: -12.0,
            y: -10.0,
            width: 64.0,
            height: 56.0
        }
    );
    let p = r.paint(&s, Theme::Light, 1.0);
    assert_eq!((p.width(), p.height()), (64, 56));
    // The red child, at (30..50, -10..0), drawn at its place in the PNG.
    assert!(close(rgba(&p, 12 + 45, 5), [255, 0, 0, 255]));
    // The shadow, below the box.
    assert!(rgba(&p, 12 + 20, 10 + 30 + 3)[3] > 100);
    // A clipping parent cuts what its children paint, and the area too.
    let s = scene(
        r##"<Frame width={40} height={30} clip><Rect x={30} width={50} height={10} fill="#ff0000" /></Frame>"##,
    );
    assert_eq!(
        r.drawing(&s, Theme::Light, None).unwrap().area().width,
        40.0
    );
}

#[test]
fn a_drawing_of_any_size_is_a_picture() {
    let mut r = Renderer::new();
    let s = scene(r##"<Frame width={70000} height={10} fill="#000000" />"##);
    let (px, w, h) = pixels(&crate::png(&mut r, &s, Theme::Light).unwrap()).unwrap();
    assert_eq!((w, h), (70000, 10));
    assert_eq!(&px[px.len() - 4..], &[0, 0, 0, 255]);
}

/// Files whose `.png`s are image files of the size their name says,
/// `w`x`h`.png — made without decoding, their pixels never touched.
struct Sized(HashMap<String, String>);

impl crate::Files for Sized {
    fn bytes(&self, path: &str) -> Option<Vec<u8>> {
        self.0.get(path).map(|s| s.clone().into_bytes())
    }
    fn list(&self, path: &str) -> Option<Vec<String>> {
        self.0.list(path)
    }
    fn image(&self, path: &str) -> Option<Result<std::sync::Arc<scene::ImageFile>, String>> {
        let name = path.strip_suffix(".png")?;
        let (size, n) = name.split_once('-')?;
        let (w, h) = size.split_once('x')?;
        let (w, h): (u32, u32) = (w.parse().ok()?, h.parse().ok()?);
        Some(Ok(std::sync::Arc::new(scene::ImageFile {
            id: scene::ImageId::of(path.as_bytes()),
            bytes: n.as_bytes().to_vec(),
            width: w,
            height: h,
            // Zeroed pages the system hands out only once written.
            pixels: std::sync::Arc::new(vec![0; 4 * w as usize * h as usize]),
            pixels_width: w,
            pixels_height: h,
        })))
    }
}

/// Every image a drawing paints shares the GPU's one atlas. Images that
/// don't fit together are the design's problem, at the `src` of the first
/// that doesn't — never a hole in the canvas. One file shown many times
/// takes its room once.
#[test]
fn images_that_do_not_fit_the_atlas_are_a_problem_where_written() {
    let design = |files: &[&str]| {
        let images: String = files
            .iter()
            .map(|f| format!("\n  <Image src=\"{f}\" width={{40}} height={{40}} />"))
            .collect();
        Sized(
            [(
                "a.design".to_string(),
                format!(r#"<Frame layout="row">{images}</Frame>"#),
            )]
            .into(),
        )
    };
    let mut r = Renderer::new();
    let four = design(&[
        "4096x4096-a.png",
        "4096x4096-b.png",
        "4096x4096-c.png",
        "4096x4096-d.png",
    ]);
    let scene = crate::resolve(&four, "a.design").unwrap();
    assert!(r.drawing(&scene, Theme::Light, None).is_ok());
    // The same four again, and again: still four images.
    let again = design(&[
        "4096x4096-a.png",
        "4096x4096-b.png",
        "4096x4096-a.png",
        "4096x4096-c.png",
        "4096x4096-d.png",
        "4096x4096-b.png",
    ]);
    let scene = crate::resolve(&again, "a.design").unwrap();
    assert!(r.drawing(&scene, Theme::Light, None).is_ok());
    let six = design(&[
        "4096x4096-a.png",
        "4096x4096-b.png",
        "4096x4096-c.png",
        "4096x4096-d.png",
        "4096x4096-e.png",
        "4096x4096-f.png",
    ]);
    let scene = crate::resolve(&six, "a.design").unwrap();
    let Err(e) = r.drawing(&scene, Theme::Light, None) else {
        panic!("six 4096-pixel squares fit")
    };
    let at = e.at.unwrap();
    assert_eq!(
        (at.file.as_str(), at.pos.line, at.pos.col),
        ("a.design", 6, 10)
    );
    assert_eq!(
        e.message,
        "this doesn't fit: the images a drawing paints — its image files, at most 4096 \
         pixels a side, and its shadows — share the GPU's one 8192×8192 atlas, and the 4 \
         before this 4096×4096 fill it"
    );
}

/// A drawing made after another that paints the same images — files and
/// shadows alike, in the same order — paints the last one's, under the
/// same blobs, so the GPU keeps them in its atlas: an edit to text or
/// layout uploads no image again. Any other change of images, and any
/// drawing made after none, paints every image under a blob of its own.
#[test]
fn a_drawing_with_the_last_ones_images_paints_them_under_the_same_blobs() {
    let design = |text: &str, files: &[&str], blur: u32| {
        let images: String = files
            .iter()
            .map(|f| format!("\n  <Image src=\"{f}\" width={{40}} height={{40}} />"))
            .collect();
        let source = format!(
            r##"<Frame layout="row"><Text>{text}</Text>{images}
  <Rect width={{40}} height={{40}} shadow={{{{ blur: {blur}, color: "#000000" }}}} />
</Frame>"##
        );
        crate::resolve(
            &Sized([("a.design".to_string(), source)].into()),
            "a.design",
        )
        .unwrap()
    };
    let ids = |d: &Drawing| d.images.iter().map(|i| i.data.id()).collect::<Vec<_>>();
    let painted = |d: &Drawing| {
        d.list
            .chunks()
            .flat_map(|(_, c)| c.commands.iter().map(|c| &c.draw))
            .filter_map(|c| match c {
                Draw::Fill(_, Paint::Image(i, _)) => Some(i.data.id()),
                _ => None,
            })
            .collect::<std::collections::BTreeSet<_>>()
    };
    let mut r = Renderer::new();
    let files = ["300x200-a.png", "100x100-b.png", "300x200-a.png"];
    let first = r
        .drawing(&design("One", &files, 6), Theme::Light, None)
        .unwrap();
    // Two files, one shown twice, and the shadow's images.
    assert!(first.images.len() > 2);
    assert_eq!(painted(&first), ids(&first).into_iter().collect());

    // New text and a new layout: the very same images.
    let edited = r
        .drawing(&design("Two words", &files, 6), Theme::Light, Some(&first))
        .unwrap();
    assert_eq!(ids(&edited), ids(&first));
    assert_eq!(painted(&edited), painted(&first));

    // A drawing made after none, and drawings whose images differ — a
    // file, their order, a shadow — share nothing with the last.
    let fresh = r
        .drawing(&design("One", &files, 6), Theme::Light, None)
        .unwrap();
    let other = ["300x200-a.png", "100x100-c.png"];
    let swapped = ["100x100-b.png", "300x200-a.png"];
    for d in [
        fresh,
        r.drawing(&design("One", &other, 6), Theme::Light, Some(&first))
            .unwrap(),
        r.drawing(&design("One", &swapped, 6), Theme::Light, Some(&first))
            .unwrap(),
        r.drawing(&design("One", &files, 7), Theme::Light, Some(&first))
            .unwrap(),
    ] {
        let shared: Vec<_> = ids(&d)
            .into_iter()
            .filter(|i| ids(&first).contains(i))
            .collect();
        assert!(shared.is_empty(), "{shared:?}");
        assert_eq!(painted(&d), ids(&d).into_iter().collect());
    }
}

#[test]
fn no_input_makes_the_engine_panic() {
    let attrs = [
        "fill",
        "stroke",
        "strokeWidth",
        "radius",
        "shadow",
        "clip",
        "layout",
        "gap",
        "padding",
        "align",
        "justify",
        "width",
        "height",
        "x",
        "y",
        "opacity",
        "onClick",
        "size",
        "weight",
        "color",
        "lineHeight",
        "textAlign",
        "d",
        "viewBox",
        "src",
        "font",
        "name",
    ];
    let values = [
        "\"#fff\u{200b}\"",
        "\"#ffé\"",
        "\"€\"",
        "\"\"",
        "\"#\"",
        "\"#12345\"",
        "\"linear-gradient(\"",
        "\"linear-gradient(é)\"",
        "\"linear-gradient(90deg, #fff é%)\"",
        "{-1000000}",
        "{1000000}",
        "{0}",
        "{-0.0001}",
        "{[]}",
        "{[1]}",
        "{[1,2,3]}",
        "{[1000000, 1000000, 1000000, 1000000]}",
        "{{}}",
        "{{ blur: 1000000, spread: 1000000 }}",
        "{{ x: \"é\" }}",
        "{true}",
        "\"navigate \"",
        "\"navigate ../../../x.design\"",
        "\"M\"",
        "\"M0 0 L1000000 1000000 Z\"",
        "\"0 0 0 0\"",
        "\"é é é é\"",
        "\"hug\"",
        "\"fill\"",
        "\"none\"",
        "\"🙂\"",
    ];
    let mut r = Renderer::new();
    for el in ["Frame", "Text", "Rect", "Ellipse", "Vector", "Image"] {
        for attr in attrs {
            for value in values {
                let source = format!(
                    r#"<Frame layout="row"><{el} {attr}={value}>{}</{el}></Frame>"#,
                    if el == "Text" { "é 🙂" } else { "" }
                );
                let files: HashMap<String, String> =
                    [("a.design".to_string(), source.clone())].into();
                if let Ok(s) = crate::resolve(&files, "a.design") {
                    // Laid out, made into a display list and its first
                    // tile painted: a drawing a million pixels a side is
                    // drawable, and painting all of it is not the point.
                    if let Ok(d) = r.drawing(&s, Theme::Light, None) {
                        let _ = crate::cpu::tile(&d.list, 1.0, 0, 0);
                    }
                }
            }
        }
    }
    // Truncations of a real design at every character, multi-byte ones
    // included.
    let source = r##"import B from "./b.design"
<Frame props={{ t: "héllo 🙂" }} layout="column" fill="#fff">
  <Text color="#123">{t} — ok</Text><B />
</Frame>"##;
    for (i, _) in source.char_indices() {
        let files: HashMap<String, String> = [
            ("a.design".to_string(), source[..i].to_string()),
            ("b.design".to_string(), "<Frame />".to_string()),
        ]
        .into();
        let _ = crate::resolve(&files, "a.design");
    }
}

/// A click on the 100,000-pixel design is answered from the index: the
/// topmost node with an interaction under the point, and nothing where a
/// clipping frame hides it.
#[test]
fn a_click_is_answered_from_the_index() {
    let mut rows = String::new();
    for i in 0..1000 {
        rows.push_str(&format!(
            "<Frame width={{400}} height={{100}} onClick=\"navigate ./r{i}.design\"><Frame width={{50}} height={{50}} clip><Rect x={{40}} width={{100}} height={{20}} onClick=\"back\" /></Frame></Frame>\n"
        ));
    }
    let mut files: HashMap<String, String> = (0..1000)
        .map(|i| (format!("r{i}.design"), "<Frame />".to_string()))
        .collect();
    files.insert(
        "a.design".to_string(),
        format!("<Frame layout=\"column\">{rows}</Frame>"),
    );
    let s = crate::resolve(&files, "a.design").unwrap();
    let mut r = Renderer::new();
    let d = r.drawing(&s, Theme::Light, None).unwrap();
    assert_eq!(d.action_at(200.0, 99_950.0), Some("navigate r999.design"));
    // The rectangle, shown inside its clip, and hidden past it.
    assert_eq!(d.action_at(45.0, 99_905.0), Some("back"));
    assert_eq!(d.action_at(80.0, 99_905.0), Some("navigate r999.design"));
    assert_eq!(d.action_at(500.0, 99_905.0), None);
}
