use crate::Files;
use crate::render::Renderer;
use crate::render::testing::*;
use crate::scene::Theme;

use super::*;

/// The repository with one file's text replaced.
struct Edited(Repo, &'static str, String);

impl Files for Edited {
    fn bytes(&self, path: &str) -> Option<Vec<u8>> {
        if path == self.1 {
            Some(self.2.clone().into_bytes())
        } else {
            self.0.bytes(path)
        }
    }
    fn list(&self, path: &str) -> Option<Vec<String>> {
        self.0.list(path)
    }
}

/// Hiding one small part of the checkout design — painting it in the
/// color behind it, so nothing else moves — takes it outside the
/// tolerance: the comparison catches a missing part, not only a missing
/// screen.
#[test]
fn a_missing_divider_or_icon_is_outside_the_tolerance() {
    let path = "docs/designs/checkout/checkout.design";
    let original = std::fs::read_to_string(repo().0.join(path)).unwrap();
    let mut renderer = Renderer::new();
    let mut paint = |files: &dyn Files| {
        let scene = crate::resolve(files, path).unwrap_or_else(|p| panic!("{p:?}"));
        pixels(&crate::png(&mut renderer, &scene, Theme::Light).unwrap()).unwrap()
    };
    let whole = paint(&repo());
    for (part, hidden) in [
        (
            r##"<Rect name="Divider" width="fill" height={1} fill={theme.border} />"##,
            r##"<Rect name="Divider" width="fill" height={1} fill={theme.background} />"##,
        ),
        (
            r##"viewBox="0 0 24 24" width={20} height={20} fill={theme.muted}"##,
            r##"viewBox="0 0 24 24" width={20} height={20} fill={theme.card}"##,
        ),
    ] {
        assert!(original.contains(part), "{part}");
        let edited = Edited(repo(), path, original.replace(part, hidden));
        let without = paint(&edited);
        let d = matches(&whole.0, &without.0, whole.1 as usize);
        assert!(d.is_err(), "hiding {part} went unnoticed: {d:?}");
    }
}

#[test]
fn matching_tolerates_anti_aliasing_only() {
    let width = 100;
    // A dark square on white, its edge anti-aliased to grey.
    let mut a = vec![255u8; 4 * width * 100];
    for y in 20..60 {
        for x in 20..60 {
            let edge = x == 20 || x == 59 || y == 20 || y == 59;
            let v = if edge { 128 } else { 0 };
            a[4 * (y * width + x)..][..3].copy_from_slice(&[v; 3]);
        }
    }
    // Another rasterizer's edge: any grey between the two sides.
    let mut b = a.clone();
    for px in b.chunks_mut(4).filter(|p| p[0] == 128) {
        px[..3].copy_from_slice(&[40; 3]);
    }
    let d = compare(&a, &b, width).unwrap();
    assert_eq!((d.share, d.max), (0.0, 88));
    assert!(matches(&a, &b, width).is_ok());
    // A missing one-pixel divider is unexplained along its whole length.
    let mut line = a.clone();
    for x in 0..width {
        line[4 * (80 * width + x)..][..3].copy_from_slice(&[226; 3]);
    }
    let d = matches(&a, &line, width).unwrap_err().unwrap();
    assert_eq!(d.cluster, width);
    assert_eq!(matches(&a, &a[4..], width), Err(None));
}
