use std::collections::HashMap;
use std::sync::Arc;

use super::*;
use crate::Files;
use crate::cpu;
use crate::scene::{ImageFile, ImageId};

fn resolve(source: &str) -> Scene {
    let files: HashMap<String, String> = [("a.design".to_string(), source.to_string())].into();
    crate::resolve(&files, "a.design").unwrap()
}

/// The tiles of a drawing at 1×, every one, as `cpu` paints them.
fn tiles(list: &List) -> Vec<Vec<u8>> {
    let (w, h) = (list.area.width as u32, list.area.height as u32);
    let n = |v: u32| v.div_ceil(tiles::TILE);
    (0..n(h))
        .flat_map(|row| (0..n(w)).map(move |col| (col, row)))
        .map(|(col, row)| cpu::tile(list, 1.0, col, row))
        .collect()
}

/// What a fresh drawing of `scene` paints, tile by tile.
fn fresh(scene: &Scene, theme: Theme) -> Vec<Vec<u8>> {
    let mut renderer = Renderer::new();
    let drawing = renderer.drawing(scene, theme, None).unwrap();
    tiles(&drawing.list)
}

/// Every tile of `design` in `theme`, painted as a canvas or a PNG paints
/// them: one at a time, each from the list rounded where it is.
fn painted(design: &mut Design, renderer: &mut Renderer, theme: Theme) -> Vec<Vec<u8>> {
    let a = design.area(renderer, theme).unwrap();
    let n = |v: f32| (v as u32).div_ceil(tiles::TILE);
    let mut out = Vec::new();
    for row in 0..n(a.height) {
        for col in 0..n(a.width) {
            let list = design.tile(renderer, theme, 1.0, col, row).unwrap();
            out.push(cpu::tile(list, 1.0, col, row));
        }
    }
    out
}

/// A new design stales every tile of both themes; a patch damages, in each
/// theme drawn, only where the node it changed was and is, from the
/// drawing's top-left; a theme never drawn takes no damage.
#[test]
fn an_edit_damages_only_what_it_changed_in_each_theme_drawn() {
    let source = |fill: &str| {
        format!(
            r##"<Frame width={{400}} height={{2000}} fill="#ffffff">
  <Rect x={{10}} y={{10}} width={{50}} height={{50}} fill="#000000" />
  <Rect x={{10}} y={{1500}} width={{50}} height={{50}} fill="{fill}" />
</Frame>"##
        )
    };
    let (first, second) = (resolve(&source("#ff0000")), resolve(&source("#00ff00")));
    let mut renderer = Renderer::new();
    let mut design = Design::new(first.clone());
    assert_eq!(
        design.take_damage(),
        vec![(Theme::Light, Damage::All), (Theme::Dark, Damage::All)]
    );
    design.draw(&mut renderer, Theme::Light).unwrap();
    design.take_damage();
    design
        .apply(&mut renderer, &Edits::between(&first, &second))
        .unwrap();
    let damage = design.take_damage();
    let [(Theme::Light, Damage::Rects { origin, rects })] = damage.as_slice() else {
        panic!("only the light theme was drawn: {damage:?}");
    };
    assert_eq!(*origin, (0.0, 0.0));
    assert!(!rects.is_empty());
    for r in rects {
        assert!(r.y0 >= 1490.0 && r.y1 <= 1560.0, "{r:?}");
    }
    assert_eq!(
        painted(&mut design, &mut renderer, Theme::Light),
        fresh(&second, Theme::Light)
    );
}

/// Files whose `.png`s are image files of the size their name says,
/// `w`x`h`-`n`.png — their pixels zeroed, never decoded.
struct Sized(HashMap<String, String>);

impl Files for Sized {
    fn bytes(&self, path: &str) -> Option<Vec<u8>> {
        self.0.get(path).map(|s| s.clone().into_bytes())
    }
    fn list(&self, path: &str) -> Option<Vec<String>> {
        self.0.list(path)
    }
    fn image(&self, path: &str) -> Option<Result<Arc<ImageFile>, String>> {
        let name = path.strip_suffix(".png")?;
        let (size, n) = name.split_once('-')?;
        let (w, h) = size.split_once('x')?;
        let (w, h): (u32, u32) = (w.parse().ok()?, h.parse().ok()?);
        Some(Ok(Arc::new(ImageFile {
            id: ImageId::of(path.as_bytes()),
            bytes: n.as_bytes().to_vec(),
            width: w,
            height: h,
            pixels: Arc::new(vec![0; 4 * w as usize * h as usize]),
            pixels_width: w,
            pixels_height: h,
        })))
    }
}

/// Four images that fill the GPU's atlas, a fifth when `more`, and — 1,500
/// pixels below them — a box in `fill`.
fn crowded(more: bool, fill: &str) -> Scene {
    let mut images: String = (0..4)
        .map(|n| format!(r#"<Image src="4096x4096-{n}.png" width={{40}} height={{40}} />"#))
        .collect();
    if more {
        images.push_str(r#"<Image src="8x8-5.png" width={40} height={40} />"#);
    }
    let source = format!(
        r##"<Frame layout="column" gap={{4}} padding={{8}} fill="#ffffff"><Frame layout="row" gap={{4}}>{images}</Frame><Rect width={{40}} height={{1500}} fill="#ffffff" /><Rect width={{40}} height={{40}} fill="{fill}" /></Frame>"##
    );
    crate::resolve(
        &Sized([("a.design".to_string(), source)].into()),
        "a.design",
    )
    .unwrap()
}

/// A canvas's view of `design` at `zoom`, its top-left at `origin`.
fn view(zoom: f64, origin: (f64, f64)) -> View {
    View {
        width: 800.0,
        height: 600.0,
        zoom,
        origin,
    }
}

/// The tiles `view` needs of `design`, painted as a canvas paints them.
fn viewed(design: &mut Design, renderer: &mut Renderer, view: &View) -> Vec<Vec<u8>> {
    let scale = Level::of(view.zoom).scale();
    design
        .view_tiles(renderer, Theme::Light, view)
        .into_iter()
        .map(|(col, row)| {
            let list = design
                .tile(renderer, Theme::Light, scale, col, row)
                .expect("a tile to paint");
            cpu::tile(list, scale, col, row)
        })
        .collect()
}

/// An edit that leaves the scene undrawable — its images too many for the
/// GPU's atlas — says why, damages no tile, and keeps showing the last good
/// drawing: every tile a view needs paints the scene before the edit, the
/// tiles of a pan and a zoom no one painted before too. Saves in the
/// meantime change nothing shown. The edit that makes the scene drawable
/// again damages every tile, and the drawing paints as a fresh one of it.
#[test]
fn a_scene_that_cant_be_drawn_shows_the_last_good_drawing_until_it_can() {
    let (good, broken, later, again) = (
        crowded(false, "#ff0000"),
        crowded(true, "#00ff00"),
        crowded(true, "#ffaa00"),
        crowded(false, "#0000ff"),
    );
    let mut renderer = Renderer::new();
    let mut design = Design::new(good.clone());
    design.draw(&mut renderer, Theme::Light).unwrap();
    let size = design.area(&mut renderer, Theme::Light).unwrap();
    let (top, panned, zoomed) = (
        view(1.0, (0.0, 0.0)),
        view(1.0, (0.0, -1100.0)),
        view(2.0, (0.0, -2800.0)),
    );
    let mut fresh_design = Design::new(good.clone());
    let want: Vec<_> = [&top, &panned, &zoomed]
        .iter()
        .map(|v| viewed(&mut fresh_design, &mut Renderer::new(), v))
        .collect();
    // Only the top of it is painted before it breaks.
    assert_eq!(viewed(&mut design, &mut renderer, &top), want[0]);
    design.take_damage();

    design
        .apply(&mut renderer, &Edits::between(&good, &broken))
        .unwrap();
    let problem = design.problem(Theme::Light).expect("undrawable");
    assert!(problem.message.contains("doesn't fit"), "{problem}");
    assert!(design.take_damage().is_empty());
    assert!(design.draw(&mut renderer, Theme::Light).is_err());
    design
        .apply(&mut renderer, &Edits::between(&broken, &later))
        .unwrap();
    assert!(design.take_damage().is_empty());
    // Its size, its images and its tiles are the last good drawing's.
    assert_eq!(design.area(&mut renderer, Theme::Light).unwrap(), size);
    assert_eq!(design.images(&mut renderer, Theme::Light).unwrap().len(), 4);
    for (v, want) in [&top, &panned, &zoomed].into_iter().zip(&want) {
        let got = viewed(&mut design, &mut renderer, v);
        assert!(!got.is_empty());
        assert!(got == *want, "the view at {:?} ×{}", v.origin, v.zoom);
    }
    assert_eq!(design.scene(), &later);

    design
        .apply(&mut renderer, &Edits::between(&later, &again))
        .unwrap();
    assert!(design.problem(Theme::Light).is_none());
    assert_eq!(design.take_damage(), vec![(Theme::Light, Damage::All)]);
    assert_eq!(
        painted(&mut design, &mut renderer, Theme::Light),
        fresh(&again, Theme::Light)
    );
}

/// A whole scene that can't be drawn, after one that could, keeps showing
/// the drawing of the one before; a whole scene that can be drawn then
/// damages every tile again, and is what is painted.
#[test]
fn a_whole_scene_that_cant_be_drawn_shows_the_one_before() {
    let (good, broken, again) = (
        crowded(false, "#ff0000"),
        crowded(true, "#00ff00"),
        crowded(false, "#0000ff"),
    );
    let mut renderer = Renderer::new();
    let mut design = Design::new(good.clone());
    design.draw(&mut renderer, Theme::Light).unwrap();
    design.take_damage();
    design.replace(&mut renderer, broken);
    design.take_damage();
    assert!(design.draw(&mut renderer, Theme::Light).is_err());
    assert_eq!(
        painted(&mut design, &mut renderer, Theme::Light),
        fresh(&good, Theme::Light)
    );
    design.replace(&mut renderer, again.clone());
    assert!(design.problem(Theme::Light).is_none());
    design.draw(&mut renderer, Theme::Light).unwrap();
    assert!(design.take_damage().contains(&(Theme::Light, Damage::All)));
    assert_eq!(
        painted(&mut design, &mut renderer, Theme::Light),
        fresh(&again, Theme::Light)
    );
}

/// A whole scene that can't be drawn has no drawing to show: drawing it
/// says why, and isn't tried again until the scene changes. A whole scene
/// that can be drawn then is.
#[test]
fn a_whole_scene_that_cant_be_drawn_is_tried_again_only_when_it_changes() {
    let mut renderer = Renderer::new();
    let mut design = Design::new(crowded(true, "#00ff00"));
    assert!(design.draw(&mut renderer, Theme::Light).is_err());
    assert!(design.problem(Theme::Light).is_some());
    assert!(design.area(&mut renderer, Theme::Light).is_none());
    design.take_damage();
    let good = crowded(false, "#ff0000");
    design.replace(&mut renderer, good.clone());
    assert!(design.problem(Theme::Light).is_none());
    design.draw(&mut renderer, Theme::Light).unwrap();
    assert_eq!(
        painted(&mut design, &mut renderer, Theme::Light),
        fresh(&good, Theme::Light)
    );
}

/// A whole scene stales every tile of both themes and makes each drawing
/// afresh when it is next asked for; a theme let go takes its damage with
/// it, and a patch leaves it undrawn.
#[test]
fn a_whole_scene_redraws_and_a_theme_let_go_is_left_alone() {
    let themed = |bg: &str| {
        let files: HashMap<String, String> = [
            (
                "themes.yml".to_string(),
                format!("kind: themes\nlight:\n  bg: \"{bg}\"\ndark:\n  bg: \"#000000\"\n"),
            ),
            (
                "a.design".to_string(),
                "<Frame width={10} height={10} fill={theme.bg} />".to_string(),
            ),
        ]
        .into();
        crate::resolve(&files, "a.design").unwrap()
    };
    let (white, grey) = (themed("#ffffff"), themed("#808080"));
    let mut renderer = Renderer::new();
    let mut design = Design::new(white.clone());
    for theme in THEMES {
        design.draw(&mut renderer, theme).unwrap();
    }
    design.take_damage();
    design.replace(&mut renderer, grey.clone());
    assert_eq!(
        design.take_damage(),
        vec![(Theme::Light, Damage::All), (Theme::Dark, Damage::All)]
    );
    assert_eq!(
        painted(&mut design, &mut renderer, Theme::Light),
        fresh(&grey, Theme::Light)
    );
    design.forget(Theme::Dark);
    design.take_damage();
    design
        .apply(&mut renderer, &Edits::between(&grey, &white))
        .unwrap();
    let damage = design.take_damage();
    assert!(damage.iter().all(|(t, _)| *t == Theme::Light), "{damage:?}");
    assert_eq!(
        painted(&mut design, &mut renderer, Theme::Dark),
        fresh(&white, Theme::Dark)
    );
}
