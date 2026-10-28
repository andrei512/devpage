//! The engine end to end on the repository's example designs and on one
//! 100,000 pixels tall: against their reference PNGs, and tile by tile.
use std::path::PathBuf;

use std::collections::HashMap;

use crate::render::testing::*;
use crate::render::*;
use crate::resolve::{self, Theme};

/// Each example design, painted natively in each theme, matches its
/// reference PNG in `design/testdata/` — `<name>.png` in the light theme,
/// `<name>-dark.png` in the dark one — within the tolerance [`matches`]
/// applies. After a deliberate change to
/// how designs look, `DEVPAGE_UPDATE_REFERENCES=1 cargo test` rewrites
/// them — and the diff shows the new drawings for review.
#[test]
fn the_examples_paint_as_their_references() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let update = std::env::var_os("DEVPAGE_UPDATE_REFERENCES").is_some();
    let mut renderer = Renderer::new();
    for (theme, suffix) in [(Theme::Light, ""), (Theme::Dark, "-dark")] {
        for name in ["button", "checkout", "confirmation", "details"] {
            let path = format!("docs/designs/checkout/{name}.design");
            let scene = crate::resolve(&repo(), &path).unwrap_or_else(|p| panic!("{p:?}"));
            assert!(scene.themed, "{name}");
            let png = crate::png(&mut renderer, &scene, theme).unwrap();
            let name = format!("{name}{suffix}");
            let reference = dir.join(format!("{name}.png"));
            if update {
                std::fs::write(&reference, &png).unwrap();
                continue;
            }
            let expected = resolve::decode(&std::fs::read(&reference).unwrap()).unwrap();
            let painted = resolve::decode(&png).unwrap();
            assert_eq!(
                (painted.width, painted.height),
                (expected.width, expected.height),
                "{name}"
            );
            if let Err(d) = matches(
                painted.data.data(),
                expected.data.data(),
                painted.width as usize,
            ) {
                panic!("{name}: {d:?}");
            }
        }
    }
}

/// The examples, painted tile by tile and streamed band by band, are the
/// references the whole drawing painted in one went into, to within 2 of
/// 255 in any channel; and a tile is that part of one paint of the whole.
#[test]
fn tiles_paint_the_examples_as_the_whole_drawing() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let mut renderer = Renderer::new();
    for (theme, suffix) in [(Theme::Light, ""), (Theme::Dark, "-dark")] {
        for name in ["button", "checkout", "confirmation", "details"] {
            let path = format!("docs/designs/checkout/{name}.design");
            let scene = crate::resolve(&repo(), &path).unwrap();
            let tiled = crate::png(&mut renderer, &scene, theme).unwrap();
            let whole = std::fs::read(dir.join(format!("{name}{suffix}.png"))).unwrap();
            let (a, w, h) = pixels(&whole).unwrap();
            let (b, tw, th) = pixels(&tiled).unwrap();
            assert_eq!((w, h), (tw, th), "{name}");
            let d = compare(&a, &b, w as usize).unwrap();
            assert!(d.within_tolerance(), "{name} {theme:?}: {d:?}");
            assert!(d.max <= 2, "{name} {theme:?}: {d:?}");
            // One paint of the whole drawing, and its first tile.
            let drawing = renderer.drawing(&scene, theme, None).unwrap();
            let one = paint_area(&drawing.list, frame_rect(drawing.area()), 1.0);
            let mut tile = crate::tiles::tile(&drawing.list, 1.0, 0, 0);
            let mut whole = one.data_as_u8_slice().to_vec();
            crate::tiles::unpremultiply(&mut whole);
            let side = crate::tiles::TILE as usize;
            for y in 0..side.min(h as usize) {
                let n = side.min(w as usize) * 4;
                let row = &whole[y * w as usize * 4..][..n];
                let tr = &mut tile[y * side * 4..][..n];
                for (p, q) in row.iter().zip(tr.iter()) {
                    assert!(p.abs_diff(*q) <= 1, "{name} tile row {y}");
                }
            }
        }
    }
}

/// A design 100,000 pixels tall is drawn: its PNG streams band by band.
#[test]
fn a_design_100000_pixels_tall_has_a_png() {
    let mut rows = String::new();
    for i in 0..1000 {
        rows.push_str(&format!(
            "<Frame width={{400}} height={{100}} fill=\"#{:06x}\"><Text size={{20}}>Row {i}</Text></Frame>\n",
            (i * 7919) % 0xffffff
        ));
    }
    let files: HashMap<String, String> = [(
        "a.design".to_string(),
        format!("<Frame layout=\"column\" fill=\"#ffffff\">{rows}</Frame>"),
    )]
    .into();
    let s = resolve::resolve(&files, "a.design").unwrap();
    let mut r = Renderer::new();
    let t = std::time::Instant::now();
    let drawing = r.drawing(&s, Theme::Light, None).unwrap();
    let list = &drawing.list;
    let built = t.elapsed();
    assert_eq!((list.area.width, list.area.height), (400.0, 100_000.0));
    let t = std::time::Instant::now();
    let mut png = Vec::new();
    crate::tiles::png(list, &mut png).unwrap();
    eprintln!(
        "built {built:?}, png {:?}, {} bytes",
        t.elapsed(),
        png.len()
    );
    let (px, w, h) = pixels(&png).unwrap();
    assert_eq!((w, h), (400, 100_000));
    // Row 999's fill at its middle.
    let c = 999 * 7919;
    let at = ((99_950 * 400 + 390) * 4) as usize;
    assert_eq!(&px[at..at + 3], &[(c >> 16) as u8, (c >> 8) as u8, c as u8]);
    // A tile at the bottom paints only what touches it.
    let t = std::time::Instant::now();
    let found = list
        .query(crate::tiles::tile_rect(list, 1.0, 0, 390), 1.0)
        .len();
    eprintln!("query {:?}, {found} commands", t.elapsed());
    assert!(found < 12, "{found}");
}
