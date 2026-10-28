//! The design engine for the browser. Each canvas holds a `Design`: the scene
//! its watch stream built, laid out once per change, painted on the GPU by
//! `Gpu` or on the CPU by `Design::paint`, and hit-tested for clicks.

use devpage_design::proto::{Update, update::Body};
use devpage_design::render::{Drawing, Frame, paint_list};
use devpage_design::resolve::Node;
use devpage_design::{Renderer, Scene, Theme, wire};
use prost::Message;
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
mod gpu;
#[cfg(any(target_arch = "wasm32", test))]
mod scene;

/// What one watch-stream message did to a design.
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    /// Nothing the canvas has to show.
    Nothing = 0,
    /// The scene changed: paint again.
    Changed = 1,
    /// The design has problems; the last scene is kept.
    Problems = 2,
}

/// One canvas's design: its scene, its problems, and its layout while the
/// scene stands.
#[wasm_bindgen]
pub struct Design {
    engine: Renderer,
    scene: Option<Scene>,
    problems: Vec<String>,
    /// The scene laid out and turned into its display list, once per change.
    /// The page sees only the drawing, with its top-left at `(0, 0)`.
    laid_out: Option<Drawing>,
    /// The drawing as chunks under property trees, with its index: what
    /// tiles are painted from.
    list: Option<devpage_design::display::List>,
    /// Whether the scene or the theme changed since `laid_out` was made,
    /// which is then kept for the next drawing to paint its images if they
    /// are the same.
    stale: bool,
    /// The theme the page draws this design in.
    theme: Theme,
}

/// A CPU painting: premultiplied alpha already undone, ready for `ImageData`.
#[wasm_bindgen]
pub struct Pixels {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

#[wasm_bindgen]
impl Pixels {
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The pixels, moved out to the page.
    pub fn take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.rgba)
    }
}

impl Default for Design {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl Design {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Design {
        Design {
            engine: Renderer::new(),
            scene: None,
            problems: Vec::new(),
            laid_out: None,
            list: None,
            stale: false,
            theme: Theme::Light,
        }
    }

    /// Draws the design in its dark theme, or its light one; true when that
    /// is a change, which the next paint shows. The scene holds both, so
    /// nothing is asked of the server.
    pub fn set_theme(&mut self, dark: bool) -> bool {
        let theme = if dark { Theme::Dark } else { Theme::Light };
        if theme == self.theme {
            return false;
        }
        self.theme = theme;
        self.stale = true;
        true
    }

    /// Whether any `theme.` reference reached the scene: what puts an
    /// embed's own theme switch on its backdrop.
    pub fn themed(&self) -> bool {
        self.scene.as_ref().is_some_and(|s| s.themed)
    }

    /// Applies one watch-stream message: a whole scene, a patch or problems.
    pub fn apply(&mut self, bytes: &[u8]) -> Applied {
        let Ok(update) = Update::decode(bytes) else {
            return Applied::Nothing;
        };
        let applied = match update.body {
            Some(Body::Scene(whole)) => {
                // Image and font files the canvas already holds aren't
                // decoded or checked again.
                let (images, fonts) = self
                    .scene
                    .take()
                    .map(|s| (s.images, s.fonts))
                    .unwrap_or_default();
                wire::decode_scene(&whole, (&images, &fonts)).map(|scene| self.scene = Some(scene))
            }
            Some(Body::Patch(patch)) => match self.scene.as_mut() {
                Some(scene) => wire::apply(scene, &patch),
                None => return Applied::Nothing,
            },
            Some(Body::Problems(problems)) => {
                self.problems = problems.problems;
                return Applied::Problems;
            }
            None => return Applied::Nothing,
        };
        if let Err(e) = applied {
            self.problems = vec![format!(
                "the watch stream sent what the engine can't use: {e}"
            )];
            return Applied::Problems;
        }
        self.problems.clear();
        self.stale = true;
        Applied::Changed
    }

    /// The problems the last message reported, one per line, until a scene
    /// or a patch arrives.
    pub fn problems(&self) -> Vec<String> {
        self.problems.clone()
    }

    /// The size of the design's drawing in its own pixels, `[width, height]`,
    /// or empty before the first scene.
    pub fn size(&mut self) -> Vec<f32> {
        match self.laid_out() {
            Some(l) => vec![l.area.width, l.area.height],
            None => Vec::new(),
        }
    }

    /// The interaction of the topmost node at `(x, y)`, in the drawing's
    /// pixels, that has one.
    pub fn action_at(&mut self, x: f32, y: f32) -> Option<String> {
        let root = self.scene.as_ref()?.root.clone();
        let laid_out = self.laid_out()?;
        let (x, y) = (x + laid_out.area.x, y + laid_out.area.y);
        let mut found = None;
        hit(&root, &laid_out.frames, &mut 0, x, y, None, &mut found);
        found
    }

    /// Paints tile `(col, row)` of the drawing at `scale` on the CPU with
    /// `vello_cpu`: 256 device pixels a side, straight alpha.
    pub fn paint_tile(&mut self, scale: f32, col: u32, row: u32) -> Option<Pixels> {
        let list = self.list()?;
        let rgba = devpage_design::tiles::tile(list, scale as f64, col, row);
        Some(Pixels {
            width: devpage_design::tiles::TILE,
            height: devpage_design::tiles::TILE,
            rgba,
        })
    }

    /// Paints the design on the CPU with `vello_cpu`, at `scale` device
    /// pixels per design pixel.
    pub fn paint(&mut self, scale: f32) -> Option<Pixels> {
        let laid_out = self.laid_out()?;
        let pixmap = paint_list(laid_out.area, &laid_out.list, scale);
        let mut rgba = pixmap.data_as_u8_slice().to_vec();
        for px in rgba.as_chunks_mut::<4>().0 {
            let a = px[3] as u32;
            if a != 0 && a != 255 {
                for c in &mut px[..3] {
                    *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        Some(Pixels {
            width: pixmap.width() as u32,
            height: pixmap.height() as u32,
            rgba,
        })
    }
}

impl Design {
    /// The drawing, laid out once per change — painting the last drawing's
    /// images when it paints the same ones, so the GPU keeps them. A drawing
    /// that can't be painted — too big, or its images too many for the GPU —
    /// has none; the server reports it as the design's problem.
    fn list(&mut self) -> Option<&devpage_design::display::List> {
        self.laid_out()?;
        self.list.as_ref()
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fn theme(&self) -> Theme {
        self.theme
    }

    fn laid_out(&mut self) -> Option<&Drawing> {
        if self.stale || self.laid_out.is_none() {
            self.stale = false;
            let scene = self.scene.as_ref()?;
            let last = self.laid_out.take();
            self.laid_out = self.engine.drawing(scene, self.theme, last.as_ref()).ok();
            self.list = self.laid_out.as_ref().map(|d| {
                let frames = d.frames.clone();
                self.engine.chunked(scene, &frames, self.theme)
            });
        }
        self.laid_out.as_ref()
    }
}

/// Walks in paint order, so the last hit is the topmost. Inside a clipping
/// frame only what it shows can be clicked, as only that is painted.
fn hit(
    node: &Node,
    frames: &[Frame],
    at: &mut usize,
    x: f32,
    y: f32,
    clip: Option<Frame>,
    found: &mut Option<String>,
) {
    let f = frames[*at];
    *at += 1;
    let inside = |r: Frame| x >= r.x && y >= r.y && x < r.x + r.width && y < r.y + r.height;
    let shown = clip.is_none_or(inside);
    if let Some(action) = &node.on_click
        && shown
        && inside(f)
    {
        *found = Some(action.clone());
    }
    let clip = if node.clip {
        Some(clip.map_or(f, |c| intersect(c, f)))
    } else {
        clip
    };
    for child in &node.children {
        hit(child, frames, at, x, y, clip, found);
    }
}

fn intersect(a: Frame, b: Frame) -> Frame {
    let (x0, y0) = (a.x.max(b.x), a.y.max(b.y));
    let (x1, y1) = (
        (a.x + a.width).min(b.x + b.width),
        (a.y + a.height).min(b.y + b.height),
    );
    Frame {
        x: x0,
        y: y0,
        width: (x1 - x0).max(0.0),
        height: (y1 - y0).max(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use devpage_design::proto::{Patch, Problems, Replace};
    use std::collections::HashMap;

    fn files() -> HashMap<String, String> {
        HashMap::from([
            (
                "button.design".to_string(),
                r##"<Frame props={{ label: "Button" }} layout="row" padding={[12, 20]} fill="#2563eb">
  <Text color="#ffffff">{label}</Text>
</Frame>"##
                    .to_string(),
            ),
            (
                "screen.design".to_string(),
                r##"import Button from "./button.design"
<Frame width={200} height={120} fill="#ffffff" layout="column" padding={10}>
  <Button label="Go" onClick="navigate ./next.design" />
</Frame>"##
                    .to_string(),
            ),
            ("next.design".to_string(), "<Frame />".to_string()),
        ])
    }

    fn scene_message() -> Vec<u8> {
        let scene = devpage_design::resolve(&files(), "screen.design").unwrap();
        Update {
            body: Some(Body::Scene(wire::scene(&scene))),
            ..Default::default()
        }
        .encode_to_vec()
    }

    /// Files as bytes, so a design can be set in a font file.
    struct Bytes(HashMap<String, Vec<u8>>);

    impl devpage_design::Files for Bytes {
        fn bytes(&self, path: &str) -> Option<Vec<u8>> {
            self.0.get(path).cloned()
        }
        fn list(&self, path: &str) -> Option<Vec<String>> {
            devpage_design::resolve::listed(self.0.keys(), path)
        }
    }

    /// A design set in a font file comes over the stream, in its first
    /// update and in a patch that brings another, and the canvas lays it
    /// out and paints it as the server paints its PNG: the same line breaks,
    /// the same pixels.
    #[test]
    fn a_design_in_a_font_file_paints_as_the_server_does() {
        let figtree = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/designs/checkout/fonts/Figtree/Figtree-Variable.ttf"
        ))
        .unwrap();
        let resolve = |weight: u16| {
            let files = Bytes(HashMap::from([
                ("f.ttf".to_string(), figtree.clone()),
                (
                    "themes.yml".to_string(),
                    b"kind: themes\nfonts:\n  figtree: ./f.ttf\nlight: {}\ndark: {}\n".to_vec(),
                ),
                (
                    "a.design".to_string(),
                    format!(
                        r##"<Frame width={{160}} layout="column" padding={{8}} fill="#ffffff"><Text width="fill" font="figtree" weight={{{weight}}}>Washed linen with a relaxed fit. Machine wash cold.</Text></Frame>"##
                    )
                    .into_bytes(),
                ),
            ]));
            devpage_design::resolve(&files, "a.design").unwrap()
        };
        let (first, heavier) = (resolve(400), resolve(800));
        let mut design = Design::new();
        let whole = Update {
            body: Some(Body::Scene(wire::scene(&first))),
            ..Default::default()
        };
        assert_eq!(design.apply(&whole.encode_to_vec()), Applied::Changed);
        let mut server = Renderer::new();
        for scene in [&first, &heavier] {
            if scene == &heavier {
                let patch = Update {
                    body: Some(Body::Patch(wire::patch(&first, &heavier))),
                    ..Default::default()
                };
                assert_eq!(design.apply(&patch.encode_to_vec()), Applied::Changed);
            }
            let frames = server.layout(scene);
            assert_eq!(design.laid_out().unwrap().frames, frames);
            let png = devpage_design::png(&mut server, scene, Theme::Light).unwrap();
            let (expected, w, h) = devpage_design::render::pixels(&png).unwrap();
            let mut painted = design.paint(1.0).unwrap();
            assert_eq!((painted.width(), painted.height()), (w, h));
            assert_eq!(painted.take(), expected);
        }
    }

    #[test]
    fn a_scene_is_sized_painted_and_hit_tested() {
        let mut design = Design::new();
        assert!(design.size().is_empty());
        assert_eq!(design.apply(&scene_message()), Applied::Changed);
        assert_eq!(design.size(), vec![200.0, 120.0]);
        // Inside the button (padding 10), and outside it.
        assert_eq!(
            design.action_at(15.0, 15.0).as_deref(),
            Some("navigate next.design")
        );
        assert_eq!(design.action_at(190.0, 110.0), None);
        let mut pixels = design.paint(2.0).unwrap();
        assert_eq!((pixels.width(), pixels.height()), (400, 240));
        let rgba = pixels.take();
        assert_eq!(&rgba[..4], &[255, 255, 255, 255]);
    }

    #[test]
    fn problems_keep_the_scene_and_a_patch_clears_them() {
        let mut design = Design::new();
        design.apply(&scene_message());
        let problems = Update {
            body: Some(Body::Problems(Problems {
                problems: vec!["screen.design:1:1: broken".into()],
            })),
            ..Default::default()
        }
        .encode_to_vec();
        assert_eq!(design.apply(&problems), Applied::Problems);
        assert_eq!(design.problems(), vec!["screen.design:1:1: broken"]);
        assert_eq!(design.size(), vec![200.0, 120.0]);

        // The root replaced by a wider one: laid out again, problems gone.
        let mut wider = devpage_design::resolve(&files(), "screen.design").unwrap();
        wider.root.width = devpage_design::resolve::Size::Px(300.0);
        let patch = Update {
            body: Some(Body::Patch(Patch {
                replace: vec![Replace {
                    id: "0".to_string(),
                    node: Some(wire::encode(&wider.root)),
                }],
                images: vec![],
                fonts: vec![],
                themed: false,
            })),
            ..Default::default()
        }
        .encode_to_vec();
        assert_eq!(design.apply(&patch), Applied::Changed);
        assert!(design.problems().is_empty());
        assert_eq!(design.size(), vec![300.0, 120.0]);
    }

    #[test]
    fn a_theme_switch_paints_the_scene_again_in_the_other_theme() {
        let files = HashMap::from([
            (
                "themes.yml".to_string(),
                "kind: themes\nlight:\n  bg: \"#ffffff\"\ndark:\n  bg: \"#000000\"\n".to_string(),
            ),
            (
                "a.design".to_string(),
                "<Frame width={10} height={10} fill={theme.bg} />".to_string(),
            ),
        ]);
        let scene = devpage_design::resolve(&files, "a.design").unwrap();
        let message = Update {
            body: Some(Body::Scene(wire::scene(&scene))),
            ..Default::default()
        }
        .encode_to_vec();
        let mut design = Design::new();
        assert!(!design.themed());
        design.apply(&message);
        assert!(design.themed());
        assert_eq!(
            &design.paint(1.0).unwrap().take()[..4],
            &[255, 255, 255, 255]
        );
        assert!(design.set_theme(true));
        assert!(!design.set_theme(true), "already dark");
        assert_eq!(&design.paint(1.0).unwrap().take()[..4], &[0, 0, 0, 255]);
        assert!(design.set_theme(false));
        assert_eq!(
            &design.paint(1.0).unwrap().take()[..4],
            &[255, 255, 255, 255]
        );
    }

    #[test]
    fn a_patch_before_any_scene_or_garbage_does_nothing() {
        let mut design = Design::new();
        let patch = Update {
            body: Some(Body::Patch(Patch {
                replace: vec![],
                images: vec![],
                fonts: vec![],
                themed: false,
            })),
            ..Default::default()
        }
        .encode_to_vec();
        assert_eq!(design.apply(&patch), Applied::Nothing);
        assert_eq!(design.apply(&[0xff, 0xff, 0xff]), Applied::Nothing);
        assert!(design.paint(1.0).is_none());
    }

    #[test]
    fn a_click_lands_only_on_what_a_clip_shows() {
        let files = HashMap::from([(
            "clip.design".to_string(),
            r##"<Frame width={100} height={100}>
  <Frame width={50} height={50} clip>
    <Rect x={0} y={0} width={100} height={100} onClick="back" />
  </Frame>
</Frame>"##
                .to_string(),
        )]);
        let scene = devpage_design::resolve(&files, "clip.design").unwrap();
        let message = Update {
            body: Some(Body::Scene(wire::scene(&scene))),
            ..Default::default()
        }
        .encode_to_vec();
        let mut design = Design::new();
        design.apply(&message);
        assert_eq!(design.action_at(10.0, 10.0).as_deref(), Some("back"));
        // Outside the clip the rectangle isn't painted, so it isn't clicked.
        assert_eq!(design.action_at(80.0, 80.0), None);
    }
}
