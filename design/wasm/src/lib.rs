//! The design engine for the browser. Each canvas holds a `Design`: the scene
//! its watch stream built and the drawing made from it — updated by each
//! patch — whose tiles `Gpu` paints and composites on the GPU, or a worker
//! paints on the CPU with `Design::paint_tile`, and which answers clicks.

use devpage_design::proto::{Update, update::Body};
use devpage_design::{Drawing, Renderer, Scene, Theme, wire};
use prost::Message;
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
mod gpu;
#[cfg(any(target_arch = "wasm32", test))]
mod vello_backend;

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

/// One canvas's design: its scene, its problems, its drawing in each theme
/// it has been shown in — each updated by every patch — and the damage those
/// updates did, until the canvas takes it.
#[wasm_bindgen]
pub struct Design {
    engine: Renderer,
    /// Which design this is, among every one the page has made: a canvas's
    /// tiles are of one design.
    id: u32,
    scene: Option<Scene>,
    /// How many of the scene's nodes use each of its files, so a patch
    /// drops a file no node uses without walking the scene.
    uses: wire::Uses,
    problems: Vec<String>,
    /// The scene laid out and made into its display list, in the light theme
    /// and the dark one; `None` before it is first shown in that theme, or
    /// when the scene can't be drawn — the server reports that as the
    /// design's problem.
    drawings: [Option<Drawing>; 2],
    /// Whether a theme's drawing must be made afresh: a whole scene arrived.
    stale: [bool; 2],
    /// What changed since the canvas last took it, per theme.
    damage: Vec<(Theme, Damage)>,
    /// The theme the page draws this design in.
    theme: Theme,
}

/// What an update did to the tiles of one theme of a design.
#[derive(Clone, Debug, PartialEq)]
pub enum Damage {
    /// Every tile: a new scene, or the drawing's top-left moved.
    All,
    /// The tiles over these rects, in the design's coordinates, of a
    /// drawing whose top-left is `origin`.
    Rects {
        origin: (f64, f64),
        rects: Vec<vello::kurbo::Rect>,
    },
}

fn index(theme: Theme) -> usize {
    match theme {
        Theme::Light => 0,
        Theme::Dark => 1,
    }
}

const THEMES: [Theme; 2] = [Theme::Light, Theme::Dark];

static NEXT_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

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
            id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            scene: None,
            uses: wire::Uses::default(),
            problems: Vec::new(),
            drawings: [None, None],
            stale: [false, false],
            damage: Vec::new(),
            theme: Theme::Light,
        }
    }

    /// Draws the design in its dark theme, or its light one; true when that
    /// is a change, which the next paint shows. The scene holds both, and
    /// the drawing in the other theme is kept, so switching back makes
    /// nothing and its tiles are still good.
    pub fn set_theme(&mut self, dark: bool) -> bool {
        let theme = if dark { Theme::Dark } else { Theme::Light };
        if theme == self.theme {
            return false;
        }
        self.theme = theme;
        true
    }

    /// The damage since the last call, flat for the page: per theme, its
    /// theme (0 light, 1 dark), 1 when every tile is stale or else 0, the
    /// drawing's top-left `x` and `y`, the number of rects, and each rect's
    /// `x0, y0, x1, y1` in the design's coordinates.
    pub fn take_damage(&mut self) -> Vec<f64> {
        let mut out = Vec::new();
        for (theme, damage) in self.damage.drain(..) {
            out.push(index(theme) as f64);
            match damage {
                Damage::All => out.extend([1.0, 0.0, 0.0, 0.0]),
                Damage::Rects { origin, rects } => {
                    out.extend([0.0, origin.0, origin.1, rects.len() as f64]);
                    for r in rects {
                        out.extend([r.x0, r.y0, r.x1, r.y1]);
                    }
                }
            }
        }
        out
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
                wire::decode_scene(&whole, (&images, &fonts)).map(|scene| {
                    self.uses = wire::Uses::of(&scene.root);
                    self.scene = Some(scene);
                    self.stale = [true, true];
                    for theme in THEMES {
                        self.damage.push((theme, Damage::All));
                    }
                })
            }
            Some(Body::Patch(patch)) => match self.scene.as_mut() {
                Some(scene) => {
                    // The patch's work only: what it did, by id, handed to
                    // each drawing — never a copy of the scene.
                    wire::apply(scene, &mut self.uses, &patch).map(|applied| {
                        // Each drawing follows the patch, by what it changed.
                        for theme in THEMES {
                            let i = index(theme);
                            let Some(d) = self.drawings[i].as_mut() else {
                                continue;
                            };
                            if self.stale[i] {
                                continue;
                            }
                            let damage = match d.update(&mut self.engine, scene, &applied) {
                                Ok(damage) if damage.all => Damage::All,
                                Ok(damage) => {
                                    let a = d.area();
                                    Damage::Rects {
                                        origin: (a.x as f64, a.y as f64),
                                        rects: damage.rects,
                                    }
                                }
                                Err(_) => {
                                    self.drawings[i] = None;
                                    Damage::All
                                }
                            };
                            self.damage.push((theme, damage));
                        }
                    })
                }
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
            Some(d) => vec![d.area().width, d.area().height],
            None => Vec::new(),
        }
    }

    /// The interaction of the topmost node at `(x, y)`, in the drawing's
    /// pixels, that has one: asked of the drawing's index.
    pub fn action_at(&mut self, x: f32, y: f32) -> Option<String> {
        let area = self.laid_out()?.area();
        let (x, y) = ((x + area.x) as f64, (y + area.y) as f64);
        let d = self.prepared(vello::kurbo::Rect::new(x, y, x, y))?;
        d.action_at(x as f32, y as f32).map(str::to_string)
    }

    /// The tiles a view of `width` × `height` device pixels needs at
    /// `scale` — the drawing scaled by it, its top-left at the whole device
    /// pixel `(x, y)` — nearest the view's middle first, flat: column, row,
    /// column, row… The engine's [`View`](devpage_design::tiles::View), so
    /// the CPU's compositor asks for the very tiles the GPU's would.
    pub fn view_tiles(&mut self, width: u32, height: u32, scale: f32, x: f32, y: f32) -> Vec<u32> {
        let Some(d) = self.laid_out() else {
            return Vec::new();
        };
        let view = devpage_design::tiles::View {
            width: width as f64,
            height: height as f64,
            zoom: scale as f64,
            origin: (x.round() as f64, y.round() as f64),
        };
        d.view_tiles(&view)
            .into_iter()
            .flat_map(|(c, r)| [c, r])
            .collect()
    }

    /// Paints tile `(col, row)` of the drawing at `scale` on the CPU with
    /// `vello_cpu`: 256 device pixels a side, straight alpha.
    pub fn paint_tile(&mut self, scale: f32, col: u32, row: u32) -> Option<Pixels> {
        let rect = devpage_design::tiles::tile_rect(&self.laid_out()?.list, scale as f64, col, row);
        let d = self.prepared(rect)?;
        let rgba = devpage_design::cpu::tile(&d.list, scale as f64, col, row);
        Some(Pixels {
            width: devpage_design::tiles::TILE,
            height: devpage_design::tiles::TILE,
            rgba,
        })
    }
}

impl Design {
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fn theme(&self) -> Theme {
        self.theme
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fn id(&self) -> u32 {
        self.id
    }

    /// The damage since it was last taken.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fn drain_damage(&mut self) -> Vec<(Theme, Damage)> {
        std::mem::take(&mut self.damage)
    }

    /// The drawing in the theme it is shown in — made afresh after a whole
    /// scene, or the first time that theme is shown, painting the last
    /// drawing's images when it paints the same ones, so the GPU keeps them.
    fn laid_out(&mut self) -> Option<&Drawing> {
        let i = index(self.theme);
        if self.stale[i] || self.drawings[i].is_none() {
            self.stale[i] = false;
            let scene = self.scene.as_ref()?;
            let last = self.drawings[i]
                .take()
                .or_else(|| self.drawings[1 - i].take());
            let made = self.engine.drawing(scene, self.theme, last.as_ref()).ok();
            // The other theme's drawing, taken to share its images' blobs,
            // goes back.
            if let Some(last) = last
                && last.theme() != self.theme
            {
                self.drawings[1 - i] = Some(last);
            }
            self.drawings[i] = made;
        }
        self.drawings[i].as_ref()
    }

    /// The drawing with every run child in `rect` — in the design's
    /// coordinates — rounded where the runs now place it: what is read
    /// there is a fresh drawing's.
    pub(crate) fn prepared(&mut self, rect: vello::kurbo::Rect) -> Option<&Drawing> {
        self.laid_out()?;
        let i = index(self.theme);
        let (Some(scene), Some(d)) = (self.scene.as_ref(), self.drawings[i].as_mut()) else {
            return None;
        };
        d.prepare(&mut self.engine, scene, rect);
        self.drawings[i].as_ref()
    }
}

#[cfg(test)]
impl Design {
    /// The whole drawing painted on the CPU at `scale`, as its tiles are,
    /// in one: straight alpha.
    fn paint(&mut self, scale: f32) -> Option<Pixels> {
        let a = self.laid_out()?.area();
        let whole = vello::kurbo::Rect::new(
            a.x as f64,
            a.y as f64,
            (a.x + a.width) as f64,
            (a.y + a.height) as f64,
        );
        let d = self.prepared(whole)?;
        let a = d.area();
        let rect = vello::kurbo::Rect::new(
            a.x as f64,
            a.y as f64,
            (a.x + a.width) as f64,
            (a.y + a.height) as f64,
        );
        let pixmap = devpage_design::cpu::paint_area(&d.list, rect, scale as f64);
        let mut rgba = pixmap.data_as_u8_slice().to_vec();
        devpage_design::cpu::unpremultiply(&mut rgba);
        Some(Pixels {
            width: pixmap.width() as u32,
            height: pixmap.height() as u32,
            rgba,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use devpage_design::proto::{Op, Patch, Problems, op};
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
            assert_eq!(design.laid_out().unwrap().frames(), frames);
            let png = devpage_design::png(&mut server, scene, Theme::Light).unwrap();
            let (expected, w, h) = devpage_design::compare::pixels(&png).unwrap();
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
                ops: vec![Op {
                    op: Some(op::Op::Change(wire::encode_own(&wider.root))),
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

    /// A whole scene damages every tile of both themes; a patch damages, in
    /// each theme the design has been drawn in, only where the node it
    /// changed was and is; a theme switched back to keeps its drawing.
    #[test]
    fn a_patch_damages_only_what_it_changed() {
        let source = |fill: &str| {
            HashMap::from([(
                "a.design".to_string(),
                format!(
                    r##"<Frame width={{400}} height={{2000}} fill="#ffffff">
  <Rect x={{10}} y={{10}} width={{50}} height={{50}} fill="#000000" />
  <Rect x={{10}} y={{1500}} width={{50}} height={{50}} fill="{fill}" />
</Frame>"##
                ),
            )])
        };
        let first = devpage_design::resolve(&source("#ff0000"), "a.design").unwrap();
        let second = devpage_design::resolve(&source("#00ff00"), "a.design").unwrap();
        let mut design = Design::new();
        let whole = Update {
            body: Some(Body::Scene(wire::scene(&first))),
            ..Default::default()
        };
        design.apply(&whole.encode_to_vec());
        assert_eq!(
            design.drain_damage(),
            vec![(Theme::Light, Damage::All), (Theme::Dark, Damage::All)]
        );
        design.laid_out().unwrap();
        let patch = Update {
            body: Some(Body::Patch(wire::patch(&first, &second))),
            ..Default::default()
        };
        assert_eq!(design.apply(&patch.encode_to_vec()), Applied::Changed);
        let damage = design.drain_damage();
        assert_eq!(
            damage.len(),
            1,
            "only the light theme was drawn: {damage:?}"
        );
        let (Theme::Light, Damage::Rects { origin, rects }) = &damage[0] else {
            panic!("{damage:?}");
        };
        assert_eq!(*origin, (0.0, 0.0));
        assert!(!rects.is_empty());
        for r in rects {
            assert!(r.y0 >= 1490.0 && r.y1 <= 1560.0, "{r:?}");
        }
        // The flat form the page reads: theme, all, top-left, count, rects.
        let back = Update {
            body: Some(Body::Patch(wire::patch(&second, &first))),
            ..Default::default()
        };
        design.apply(&back.encode_to_vec());
        let flat = design.take_damage();
        assert_eq!(&flat[..4], &[0.0, 0.0, 0.0, 0.0]);
        assert_eq!(flat.len(), 5 + 4 * flat[4] as usize);
        assert!(
            flat[5..]
                .chunks(4)
                .all(|r| r[1] >= 1490.0 && r[3] <= 1560.0)
        );
        assert!(design.take_damage().is_empty());
        // Dark, then light again: the light drawing is the one kept.
        assert!(design.set_theme(true));
        design.laid_out().unwrap();
        assert!(design.drawings[0].is_some() && design.drawings[1].is_some());
        assert!(design.set_theme(false));
        let kept = design.drawings[0].as_ref().unwrap() as *const Drawing;
        assert!(std::ptr::eq(design.laid_out().unwrap(), kept));
    }

    #[test]
    fn a_patch_before_any_scene_or_garbage_does_nothing() {
        let mut design = Design::new();
        let patch = Update {
            body: Some(Body::Patch(Patch {
                ops: vec![],
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
