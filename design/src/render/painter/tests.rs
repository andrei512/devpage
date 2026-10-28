use crate::cpu::*;
use std::collections::HashMap;

use crate::render::testing::*;
use crate::resolve::{self, Kind, Theme};

use super::*;

#[test]
fn a_gradient_runs_along_its_angle() {
    let s = scene(
        r##"<Frame width={100} height={20} fill="linear-gradient(90deg, #ff0000 0%, #0000ff 100%)" />"##,
    );
    let p = Renderer::new().paint(&s, Theme::Light, 1.0);
    assert!(
        close(rgba(&p, 0, 10), [255, 0, 0, 255]),
        "{:?}",
        rgba(&p, 0, 10)
    );
    assert!(
        close(rgba(&p, 99, 10), [0, 0, 255, 255]),
        "{:?}",
        rgba(&p, 99, 10)
    );
    let down = scene(
        r##"<Frame width={20} height={100} fill="linear-gradient(180deg, #ff0000 0%, #0000ff 100%)" />"##,
    );
    let p = Renderer::new().paint(&down, Theme::Light, 1.0);
    assert!(close(rgba(&p, 10, 0), [255, 0, 0, 255]));
    assert!(close(rgba(&p, 10, 99), [0, 0, 255, 255]));
}

#[test]
fn a_stroke_is_drawn_inside_the_box() {
    let s = scene(
        r##"<Frame width={40} height={40}><Rect x={10} y={10} width={20} height={20} stroke="#ff0000" strokeWidth={4} /></Frame>"##,
    );
    let p = Renderer::new().paint(&s, Theme::Light, 1.0);
    assert!(close(rgba(&p, 11, 20), [255, 0, 0, 255]));
    assert!(close(rgba(&p, 13, 20), [255, 0, 0, 255]));
    assert_eq!(rgba(&p, 9, 20)[3], 0, "nothing outside the box");
    assert_eq!(rgba(&p, 20, 20)[3], 0, "nothing in the middle");
}

#[test]
fn each_corner_takes_its_own_radius() {
    let s = scene(r##"<Frame width={40} height={40} radius={[20, 0, 0, 0]} fill="#000000" />"##);
    let p = Renderer::new().paint(&s, Theme::Light, 1.0);
    assert_eq!(rgba(&p, 1, 1)[3], 0, "top left is rounded");
    assert_eq!(rgba(&p, 39, 0)[3], 255, "top right is square");
    assert_eq!(rgba(&p, 0, 39)[3], 255, "bottom left is square");
}

/// A shadow takes the shape of its box: every corner its own radius, each
/// widened by the spread.
#[test]
fn a_shadow_follows_each_corner_and_the_spread() {
    // The shadow's box is 55..95 × 5..45: its top-left corner rounded by
    // 10 + 5, the others by 0 + 5.
    let sharp = scene(
        r##"<Frame width={100} height={60}><Rect x={10} y={10} width={30} height={30} radius={[10, 0, 0, 0]} shadow={{ x: 50, spread: 5, color: "#000000" }} /></Frame>"##,
    );
    let p = Renderer::new().paint(&sharp, Theme::Light, 1.0);
    assert_eq!(rgba(&p, 57, 7)[3], 0, "top left is rounded by 15");
    assert_eq!(rgba(&p, 92, 7)[3], 255, "top right only by 5");
    assert_eq!(rgba(&p, 92, 42)[3], 255, "bottom right only by 5");
    assert_eq!(rgba(&p, 57, 42)[3], 255, "bottom left only by 5");
    assert_eq!(rgba(&p, 94, 5)[3], 0, "but rounded");
    // Blurred, each corner still fades by its own radius.
    let blurred = scene(
        r##"<Frame width={100} height={60}><Rect x={10} y={10} width={30} height={30} radius={[20, 0, 0, 0]} shadow={{ x: 50, blur: 6, color: "#000000" }} /></Frame>"##,
    );
    let p = Renderer::new().paint(&blurred, Theme::Light, 1.0);
    let (tl, tr) = (rgba(&p, 63, 13)[3], rgba(&p, 86, 13)[3]);
    assert!(tr > 150 && tl < 100, "top left {tl}, top right {tr}");
    let (bl, br) = (rgba(&p, 63, 36)[3], rgba(&p, 86, 36)[3]);
    assert!(
        bl.abs_diff(tr) <= 2 && br.abs_diff(tr) <= 2,
        "{bl} {br} {tr}"
    );
}

/// The alpha across a box's left edge, a pixel at a time, through the
/// middle of a `size` square box with a black shadow blurred by `blur`.
/// Only a strip around the edge is painted, so even the largest box is
/// cheap to look at.
fn shadow_edge(size: u32, blur: f32) -> Vec<u8> {
    let s = scene(&format!(
        r##"<Frame width={{{}}} height={{{}}}><Rect x={{40}} y={{40}} width={{{size}}} height={{{size}}} shadow={{{{ blur: {blur}, color: "#000000" }}}} /></Frame>"##,
        size + 80,
        size + 80
    ));
    let mut r = Renderer::new();
    let d = r.drawing(&s, Theme::Light, None).unwrap();
    let middle = 40.0 + size as f32 / 2.0;
    let strip = Frame {
        x: 0.0,
        y: middle.floor(),
        width: 80.0,
        height: 1.0,
    };
    let p = paint_area(&d.list, frame_rect(strip), 1.0);
    (0..80).map(|x| p.sample(x, 0).a).collect()
}

/// Where a rising run of alphas crosses `level`, in pixels, between pixel
/// centers.
fn crossing(alpha: &[u8], level: f32) -> f32 {
    let i = alpha.iter().position(|&a| a as f32 >= level).unwrap();
    let (a, b) = (alpha[i - 1] as f32, alpha[i] as f32);
    i as f32 - 1.0 + (level - a) / (b - a) + 0.5
}

/// A shadow is a true Gaussian at every allowed size: it fades to nothing
/// three standard deviations out, and its edge rises from 10% to 90% over
/// 2.563 standard deviations — however large the box, however slight the
/// blur.
#[test]
fn a_shadow_edge_is_the_gaussian_at_every_size() {
    for size in [100, 2000, 8000, 16000] {
        for blur in [2.0f32, 8.0] {
            let sigma = blur / 2.0;
            let alpha = shadow_edge(size, blur);
            // The box's edge is at x = 40. Two and a half standard
            // deviations out a Gaussian edge is 0.6% (1.6 of 255), and
            // past that less: no step where the shadow's image ends.
            let reach = (39.5 - 2.5 * sigma).floor() as usize + 1;
            assert!(
                alpha[..reach].iter().all(|&a| a <= 2),
                "box {size} blur {blur}: {:?}",
                &alpha[..48]
            );
            let ramp = crossing(&alpha, 0.9 * 255.0) - crossing(&alpha, 0.1 * 255.0);
            assert!(
                (ramp / sigma - 2.563).abs() < 0.2,
                "box {size} blur {blur}: 10–90% over {ramp} px, σ {sigma}: {:?}",
                &alpha[..48]
            );
            let half = crossing(&alpha, 0.5 * 255.0);
            assert!(
                (half - 40.0).abs() < 0.25,
                "box {size} blur {blur}: half way at {half}"
            );
        }
    }
}

#[test]
fn opacity_applies_to_the_subtree_as_one() {
    // Two overlapping opaque squares at half opacity: where they overlap
    // is as light as where they don't, because the group fades as one.
    let s = scene(
        r##"<Frame width={30} height={10} fill="#ffffff"><Frame opacity={0.5}>
  <Rect width={20} height={10} fill="#000000" /><Rect x={10} width={20} height={10} fill="#000000" />
</Frame></Frame>"##,
    );
    let p = Renderer::new().paint(&s, Theme::Light, 1.0);
    let (alone, overlap) = (rgba(&p, 5, 5), rgba(&p, 15, 5));
    assert!(close(alone, overlap), "{alone:?} {overlap:?}");
    assert!(alone[0] > 100 && alone[0] < 155, "{alone:?}");
}

#[test]
fn clip_cuts_children_to_the_box() {
    let s = scene(
        r##"<Frame width={40} height={40}><Frame width={20} height={20} clip>
  <Rect x={10} y={10} width={30} height={30} fill="#000000" />
</Frame></Frame>"##,
    );
    let p = Renderer::new().paint(&s, Theme::Light, 1.0);
    assert_eq!(rgba(&p, 15, 15)[3], 255);
    assert_eq!(rgba(&p, 25, 25)[3], 0);
}

#[test]
fn a_vector_fills_its_path_scaled_into_its_box() {
    // A triangle in a 10×10 view box, drawn at 40×40.
    let s = scene(
        r##"<Frame width={40} height={40}><Vector d="M0 0L10 0L0 10Z" viewBox="0 0 10 10" width={40} height={40} fill="#000000" /></Frame>"##,
    );
    let p = Renderer::new().paint(&s, Theme::Light, 1.0);
    assert_eq!(rgba(&p, 5, 5)[3], 255);
    assert_eq!(rgba(&p, 35, 35)[3], 0);
}

#[test]
fn an_image_covers_its_box() {
    // A 2×1 image, left half red and right half blue, covering a 20×20
    // box: scaled to 40×20 and centered, so the box shows the middle.
    let image = png_bytes(2, 1, |x, _| {
        if x == 0 {
            [255, 0, 0, 255]
        } else {
            [0, 0, 255, 255]
        }
    });
    let mut s = scene(r#"<Frame width={20} height={20} />"#);
    s.root.kind = Kind::Image;
    let file = std::sync::Arc::new(resolve::ImageFile::new(image).unwrap());
    s.root.image = Some(resolve::Image {
        src: "i.png".into(),
        id: file.id,
        at: Default::default(),
    });
    s.images.insert(file.id, file);
    // The image is sampled smoothly, so each side is mostly its color.
    let p = Renderer::new().paint(&s, Theme::Light, 1.0);
    let (left, right) = (rgba(&p, 1, 10), rgba(&p, 18, 10));
    assert!(left[0] > 200 && left[2] < 50, "{left:?}");
    assert!(right[2] > 200 && right[0] < 50, "{right:?}");
    assert_eq!((left[3], right[3]), (255, 255));
    assert_eq!(Renderer::new().layout(&s)[0], at(0.0, 0.0, 20.0, 20.0));
}

#[test]
fn png_and_jpeg_decode() {
    let png = resolve::decode(&png_bytes(3, 2, |_, _| [1, 2, 3, 4])).unwrap();
    assert_eq!((png.width, png.height, png.data.data().len()), (3, 2, 24));
    let jpeg = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/designs/checkout/shirt.jpg"
    ))
    .unwrap();
    let jpeg = resolve::decode(&jpeg).unwrap();
    assert_eq!((jpeg.width, jpeg.height), (160, 160));
    assert_eq!(jpeg.data.data().len(), 160 * 160 * 4);
    assert!(resolve::decode(b"\x89PNG\r\n\x1a\nbroken").is_err());
}

/// A drawing is made for a theme: each color its value there.
#[test]
fn a_drawing_takes_each_colors_value_in_its_theme() {
    let files: HashMap<String, String> = [
        (
            "themes.yml".to_string(),
            "kind: themes\nlight:\n  bg: \"#ffffff\"\n  edge: \"#ff0000\"\ndark:\n  bg: \"#000000\"\n  edge: \"#00ff00\"\n"
                .to_string(),
        ),
        (
            "a.design".to_string(),
            r##"<Frame width={40} height={20} fill={theme.bg} stroke={theme.edge} strokeWidth={4}>
  <Rect x={10} y={8} width={20} height={4} fill="linear-gradient(90deg, theme.edge 0%, theme.edge 100%)" />
</Frame>"##
                .to_string(),
        ),
    ]
    .into();
    let s = resolve::resolve(&files, "a.design").unwrap();
    let mut r = Renderer::new();
    for (theme, bg, edge) in [
        (Theme::Light, [255, 255, 255, 255], [255, 0, 0, 255]),
        (Theme::Dark, [0, 0, 0, 255], [0, 255, 0, 255]),
    ] {
        let p = r.paint(&s, theme, 1.0);
        assert_eq!(rgba(&p, 6, 6), bg, "{theme:?}");
        assert_eq!(rgba(&p, 1, 1), edge, "{theme:?}");
        assert_eq!(rgba(&p, 20, 10), edge, "{theme:?}");
    }
}

#[test]
fn paints_fills_and_text() {
    let s = scene(
        r##"<Frame width={100} height={40} fill="#ff0000" layout="row" padding={8}><Text color="#ffffff">Pay</Text></Frame>"##,
    );
    let pixmap = Renderer::new().paint(&s, Theme::Light, 1.0);
    assert_eq!((pixmap.width(), pixmap.height()), (100, 40));
    let p = pixmap.sample(90, 30);
    assert_eq!((p.r, p.g, p.b, p.a), (255, 0, 0, 255));
    // Some pixel of the text is lighter than the fill.
    assert!((8..40).any(|x| (8..28).any(|y| pixmap.sample(x, y).g > 128)));
}
