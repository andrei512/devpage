//! The design engine for the browser. Each canvas holds a `Design`: the
//! engine's [`devpage_design::Design`] — the scene its watch stream built,
//! drawn in the theme the page shows, brought along by each patch — and the
//! canvas's [frame plan](devpage_design::tiles::Plan), which the damage of
//! each patch reaches here and never leaves the module. `Gpu` carries the
//! plan out on the GPU; on the CPU the page carries it out with
//! [`Design::frame`] and [`Design::arrived`], and a worker paints the tiles
//! it asks for with `Design::paint_tile`. A `Design` also answers clicks.

use devpage_design::proto::{Update, update::Body};
use devpage_design::tiles::{self, Arrival, Level, Plan, View};
use devpage_design::{Damage, Renderer, Theme, wire};
use prost::Message;
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
mod compositor;
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

/// One canvas's design: the engine's, once a scene arrives, the problems
/// the stream last reported, the theme the page draws it in, and the plan
/// of the canvas's frames.
#[wasm_bindgen]
pub struct Design {
    engine: Renderer,
    /// Which design this is, among every one the page has made: a canvas's
    /// tiles are of one design.
    id: u32,
    design: Option<devpage_design::Design>,
    problems: Vec<String>,
    /// The theme the page draws this design in.
    theme: Theme,
    plan: Plan,
}

/// The scale tiles are painted at for a view at `scale`, in device pixels:
/// its [`Level`], to a 1/1024th. The view is shown at it, so a tile at the
/// view's own level lands pixel for pixel.
#[wasm_bindgen]
pub fn level(scale: f64) -> f64 {
    Level::of(scale).scale()
}

/// A frame's [work](devpage_design::tiles::Work), flat for the page.
#[wasm_bindgen]
pub struct Work {
    draw: Vec<f64>,
    request: Vec<f64>,
    evict: Vec<u32>,
    missing: u32,
}

#[wasm_bindgen]
impl Work {
    /// The paintings to draw, bottom first: per painting its ticket and
    /// where it lands in the view, `x0, y0, x1, y1` in device pixels.
    #[wasm_bindgen(getter)]
    pub fn draw(&self) -> Vec<f64> {
        self.draw.clone()
    }

    /// The tiles to paint, in order: per tile its ticket, its theme (0
    /// light, 1 dark), its scale, column and row.
    #[wasm_bindgen(getter)]
    pub fn request(&self) -> Vec<f64> {
        self.request.clone()
    }

    /// The tickets of the paintings let go: the page frees them.
    #[wasm_bindgen(getter)]
    pub fn evict(&self) -> Vec<u32> {
        self.evict.clone()
    }

    /// How many tiles of the view at its own scale are missing or stale.
    #[wasm_bindgen(getter)]
    pub fn missing(&self) -> u32 {
        self.missing
    }
}

impl From<tiles::Work> for Work {
    fn from(work: tiles::Work) -> Work {
        Work {
            draw: work
                .draw
                .iter()
                .flat_map(|&(t, r)| [t as f64, r.x0, r.y0, r.x1, r.y1])
                .collect(),
            request: work
                .request
                .iter()
                .flat_map(|&(t, k)| {
                    [
                        t as f64,
                        index(k.theme) as f64,
                        k.level.scale(),
                        k.col as f64,
                        k.row as f64,
                    ]
                })
                .collect(),
            evict: work.evict,
            missing: work.missing as u32,
        }
    }
}

fn index(theme: Theme) -> usize {
    match theme {
        Theme::Light => 0,
        Theme::Dark => 1,
    }
}

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
            design: None,
            problems: Vec::new(),
            theme: Theme::Light,
            plan: Plan::new(tiles::CAP, tiles::BUDGET),
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

    /// Whether any `theme.` reference reached the scene: what puts an
    /// embed's own theme switch on its backdrop.
    pub fn themed(&self) -> bool {
        self.design.as_ref().is_some_and(|d| d.scene().themed)
    }

    /// Applies one watch-stream message: a whole scene, a patch or problems.
    /// A patch that leaves the scene undrawable is a problem like any the
    /// stream reports: the canvas keeps the tiles it painted last.
    pub fn apply(&mut self, bytes: &[u8]) -> Applied {
        let applied = self.update(bytes);
        // What it changed reaches the plan: the tiles over it go stale.
        for (theme, damage) in self
            .design
            .as_mut()
            .map(devpage_design::Design::take_damage)
            .unwrap_or_default()
        {
            match damage {
                Damage::All => self.plan.damage_all(theme),
                Damage::Rects { origin, rects } => self.plan.damage(theme, origin, &rects),
            }
        }
        applied
    }

    /// The problems the last message reported, one per line, until a scene
    /// or a patch arrives.
    pub fn problems(&self) -> Vec<String> {
        self.problems.clone()
    }

    /// The size of the design's drawing in its own pixels, `[width, height]`,
    /// or empty before the first scene.
    pub fn size(&mut self) -> Vec<f32> {
        let theme = self.theme;
        match self.held().and_then(|(d, r)| d.area(r, theme)) {
            Some(a) => vec![a.width, a.height],
            None => Vec::new(),
        }
    }

    /// The interaction of the topmost node at `(x, y)`, in the drawing's
    /// pixels, that has one: asked of the drawing's index.
    pub fn action_at(&mut self, x: f32, y: f32) -> Option<String> {
        let theme = self.theme;
        let (design, r) = self.held()?;
        let area = design.area(r, theme)?;
        let (x, y) = ((x + area.x) as f64, (y + area.y) as f64);
        design.action_at(r, theme, x, y).map(str::to_string)
    }

    /// The work of a frame of a view `width` × `height` device pixels, the
    /// drawing scaled by `scale` — shown at its [`level`] — and its top-left
    /// at `(x, y)`, rounded to a whole device pixel: the plan the GPU
    /// carries out, for the page to carry out on the CPU. Once a zoom has
    /// `settled` it asks for the tiles the view still needs; `None` before
    /// there is anything to draw.
    pub fn frame(
        &mut self,
        width: u32,
        height: u32,
        scale: f64,
        x: f64,
        y: f64,
        settled: bool,
    ) -> Option<Work> {
        self.plan(width, height, scale, (x, y), settled)
            .map(Work::from)
    }

    /// The painting asked for under `ticket` has arrived: true when the
    /// page keeps it, false when it frees it — the plan no longer asks for
    /// it.
    pub fn arrived(&mut self, ticket: u32) -> bool {
        self.plan.arrived(ticket) != Arrival::Dropped
    }

    /// The tile asked for under `ticket` was painted as nothing: it is
    /// asked for again.
    pub fn missed(&mut self, ticket: u32) {
        self.plan.missed(ticket);
    }

    /// Paints tile `(col, row)` of the drawing at `scale` on the CPU with
    /// `vello_cpu`: 256 device pixels a side, straight alpha. Nothing while
    /// the scene can't be drawn.
    pub fn paint_tile(&mut self, scale: f32, col: u32, row: u32) -> Option<Pixels> {
        let theme = self.theme;
        let (design, r) = self.held()?;
        let list = design.tile(r, theme, scale as f64, col, row)?;
        let rgba = devpage_design::cpu::tile(list, scale as f64, col, row);
        Some(Pixels {
            width: devpage_design::tiles::TILE,
            height: devpage_design::tiles::TILE,
            rgba,
        })
    }
}

impl Design {
    /// Applies one watch-stream message to the engine's design.
    fn update(&mut self, bytes: &[u8]) -> Applied {
        let Ok(update) = Update::decode(bytes) else {
            return Applied::Nothing;
        };
        let applied = match update.body {
            Some(Body::Scene(whole)) => {
                // Image and font files the canvas already holds aren't
                // decoded or checked again.
                let held = self
                    .design
                    .as_ref()
                    .map(|d| (d.scene().images.clone(), d.scene().fonts.clone()))
                    .unwrap_or_default();
                wire::decode_scene(&whole, (&held.0, &held.1)).map(|scene| match &mut self.design {
                    Some(design) => design.replace(scene),
                    None => self.design = Some(devpage_design::Design::new(scene)),
                })
            }
            Some(Body::Patch(patch)) => match self.design.as_mut() {
                // The patch's work only: what it did, by id, handed to each
                // drawing — never a copy of the scene.
                Some(design) => design.patch(&mut self.engine, &patch),
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
        let undrawable = self.design.as_ref().and_then(|d| {
            [Theme::Light, Theme::Dark]
                .into_iter()
                .find_map(|theme| d.problem(theme))
        });
        if let Some(undrawable) = undrawable {
            // A scene from the stream knows no files: where it broke is
            // the server's to say.
            self.problems = vec![match &undrawable.at {
                Some(at) if !at.file.is_empty() => format!(
                    "{}:{}:{}: {}",
                    at.file, at.pos.line, at.pos.col, undrawable.message
                ),
                _ => undrawable.message.clone(),
            }];
            return Applied::Problems;
        }
        self.problems.clear();
        Applied::Changed
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fn theme(&self) -> Theme {
        self.theme
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fn id(&self) -> u32 {
        self.id
    }

    /// The plan's work for a frame, as [`Design::frame`] gives it: every
    /// held tile of the theme the view shows, and once `settled` the tiles
    /// it needs — none while the scene can't be drawn.
    pub(crate) fn plan(
        &mut self,
        width: u32,
        height: u32,
        scale: f64,
        (x, y): (f64, f64),
        settled: bool,
    ) -> Option<tiles::Work> {
        let view = &View {
            width: width.max(1) as f64,
            height: height.max(1) as f64,
            zoom: level(scale),
            origin: (x.round(), y.round()),
        };
        let theme = self.theme;
        let design = self.design.as_mut()?;
        let a = design.area(&mut self.engine, theme)?;
        let area = vello::kurbo::Rect::new(
            a.x as f64,
            a.y as f64,
            (a.x + a.width) as f64,
            (a.y + a.height) as f64,
        );
        let needed = design.view_tiles(&mut self.engine, theme, view);
        Some(self.plan.frame(theme, area, view, &needed, settled))
    }

    /// The engine's design, once a scene arrived, and the renderer that
    /// draws it.
    pub(crate) fn held(&mut self) -> Option<(&mut devpage_design::Design, &mut Renderer)> {
        Some((self.design.as_mut()?, &mut self.engine))
    }
}

#[cfg(test)]
impl Design {
    /// The whole drawing painted on the CPU at `scale`, as its tiles are,
    /// in one: straight alpha.
    fn paint(&mut self, scale: f32) -> Option<Pixels> {
        let theme = self.theme;
        let (design, r) = self.held()?;
        let list = design.whole(r, theme)?;
        let a = list.area;
        let rect = vello::kurbo::Rect::new(
            a.x as f64,
            a.y as f64,
            (a.x + a.width) as f64,
            (a.y + a.height) as f64,
        );
        let pixmap = devpage_design::cpu::paint_area(list, rect, scale as f64);
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
            devpage_design::scene::listed(self.0.keys(), path)
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
                    body: Some(Body::Patch(wire::patch(
                        &devpage_design::scene::Edits::between(&first, &heavier),
                    ))),
                    ..Default::default()
                };
                assert_eq!(design.apply(&patch.encode_to_vec()), Applied::Changed);
            }
            let frames = server.layout(scene);
            let theme = design.theme;
            let (held, r) = design.held().unwrap();
            assert_eq!(held.drawing(r, theme).unwrap().frames(), frames);
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
        wider.root.width = devpage_design::scene::Size::Px(300.0);
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

    /// Frames of a view `width` × `height` at 1×, its top-left at `(0,
    /// y)`, until the view is current, every tile asked for arriving at
    /// once, as on the GPU: the tiles asked for, as column and row.
    fn paint_view(design: &mut Design, width: u32, height: u32, y: f64) -> Vec<(u32, u32)> {
        let mut asked = Vec::new();
        loop {
            let work = design.plan(width, height, 1.0, (0.0, y), true).unwrap();
            if work.request.is_empty() {
                assert_eq!(work.missing, 0);
                return asked;
            }
            for (ticket, key) in work.request {
                assert!(design.arrived(ticket));
                asked.push((key.col, key.row));
            }
        }
    }

    /// The damage of each message reaches the canvas's plan in the module:
    /// a whole scene stales every tile; a patch, in each theme the design
    /// has been drawn in, only the tiles over where the node it changed was
    /// and is; a theme switched back to keeps its drawing and its tiles.
    #[test]
    fn a_patch_stales_only_the_tiles_over_what_it_changed() {
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
        let patch = |a, b| {
            Update {
                body: Some(Body::Patch(wire::patch(
                    &devpage_design::scene::Edits::between(a, b),
                ))),
                ..Default::default()
            }
            .encode_to_vec()
        };
        let mut design = Design::new();
        let whole = Update {
            body: Some(Body::Scene(wire::scene(&first))),
            ..Default::default()
        };
        design.apply(&whole.encode_to_vec());
        // A view 400 × 600 of rows 1,300 to 1,900: two columns, three rows.
        let mut all = paint_view(&mut design, 400, 600, -1300.0);
        all.sort();
        assert_eq!(all, vec![(0, 5), (0, 6), (0, 7), (1, 5), (1, 6), (1, 7)]);
        assert!(paint_view(&mut design, 400, 600, -1300.0).is_empty());

        // The rect at 1,500 to 1,550 lies across rows 5 and 6 of column 0.
        assert_eq!(design.apply(&patch(&first, &second)), Applied::Changed);
        let mut again = paint_view(&mut design, 400, 600, -1300.0);
        again.sort();
        assert_eq!(again, vec![(0, 5), (0, 6)]);

        // Dark, then light again: the light tiles are still current.
        assert!(design.set_theme(true));
        assert_eq!(paint_view(&mut design, 400, 600, -1300.0).len(), 6);
        assert!(design.set_theme(false));
        assert!(paint_view(&mut design, 400, 600, -1300.0).is_empty());

        // A whole scene again: every tile of both themes.
        design.apply(&whole.encode_to_vec());
        assert_eq!(paint_view(&mut design, 400, 600, -1300.0).len(), 6);
        assert!(design.set_theme(true));
        assert_eq!(paint_view(&mut design, 400, 600, -1300.0).len(), 6);
    }

    /// A patch that leaves the design undrawable — four images filling the
    /// GPU's atlas and a fifth — is the design's problem, where the stream's
    /// problems show: the canvas's plan keeps showing the tiles painted,
    /// stales none and asks for none. The patch that makes it drawable again
    /// stales every tile, and the canvas paints as the server paints its PNG.
    #[test]
    fn a_patch_that_cant_be_drawn_keeps_the_tiles_painted_until_one_can() {
        // An image file `side` pixels a side, of one `fill`.
        let png = |side: u32, fill: &str| {
            let files = HashMap::from([(
                "p.design".to_string(),
                format!(r##"<Frame width={{{side}}} height={{{side}}} fill="{fill}" />"##),
            )]);
            let scene = devpage_design::resolve(&files, "p.design").unwrap();
            devpage_design::png(&mut Renderer::new(), &scene, Theme::Light).unwrap()
        };
        let mut files: HashMap<String, Vec<u8>> = ["#dc0000", "#00c800", "#0000dc", "#dcdc00"]
            .iter()
            .enumerate()
            .map(|(n, fill)| (format!("{n}.png"), png(4096, fill)))
            .collect();
        files.insert("4.png".to_string(), png(8, "#c800c8"));
        let scene = |more: bool, fill: &str| {
            let n = if more { 5 } else { 4 };
            let images: String = (0..n)
                .map(|n| format!(r#"<Image src="{n}.png" width={{40}} height={{40}} />"#))
                .collect();
            let source = format!(
                r##"<Frame layout="row" gap={{4}} padding={{8}} fill="#ffffff">{images}<Rect width={{40}} height={{40}} fill="{fill}" /></Frame>"##
            );
            let mut files = files.clone();
            files.insert("a.design".to_string(), source.into_bytes());
            devpage_design::resolve(&Bytes(files), "a.design").unwrap()
        };
        let (good, broken, again) = (
            scene(false, "#ff0000"),
            scene(true, "#00ff00"),
            scene(false, "#0000ff"),
        );
        let message = |body| {
            Update {
                body: Some(body),
                ..Default::default()
            }
            .encode_to_vec()
        };
        let patch = |a, b| {
            message(Body::Patch(wire::patch(
                &devpage_design::scene::Edits::between(a, b),
            )))
        };
        let mut design = Design::new();
        design.apply(&message(Body::Scene(wire::scene(&good))));
        assert!(design.paint_tile(1.0, 0, 0).is_some());
        let tiles = paint_view(&mut design, 800, 600, 0.0).len();
        assert!(tiles > 0);

        assert_eq!(design.apply(&patch(&good, &broken)), Applied::Problems);
        let problems = design.problems();
        assert!(
            problems.len() == 1 && problems[0].starts_with("an image doesn't fit"),
            "{problems:?}"
        );
        assert!(design.paint_tile(1.0, 0, 0).is_none());
        // The tiles painted last are shown, current, and none is asked for.
        let kept = design.plan(800, 600, 1.0, (0.0, 0.0), true).unwrap();
        assert_eq!(
            (kept.draw.len(), kept.request.len(), kept.missing),
            (tiles, 0, 0)
        );

        assert_eq!(design.apply(&patch(&broken, &again)), Applied::Changed);
        assert!(design.problems().is_empty());
        assert_eq!(paint_view(&mut design, 800, 600, 0.0).len(), tiles);
        let png = devpage_design::png(&mut Renderer::new(), &again, Theme::Light).unwrap();
        let (expected, w, h) = devpage_design::compare::pixels(&png).unwrap();
        let mut painted = design.paint(1.0).unwrap();
        assert_eq!((painted.width(), painted.height()), (w, h));
        assert_eq!(painted.take(), expected);
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
