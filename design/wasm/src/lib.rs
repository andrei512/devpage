//! The design engine for the browser. Each canvas holds a `Design`: the scene
//! its watch stream built, laid out once per change, painted on the GPU by
//! `Gpu` or on the CPU by `Design::paint`, and hit-tested for clicks.

use devpage_design::proto::{Update, update::Body};
use devpage_design::render::{Drawing, Frame, paint_list};
use devpage_design::resolve::Node;
use devpage_design::{Renderer, Scene, wire};
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
    /// Whether the scene changed since `laid_out` was made from it, which is
    /// then kept for the next drawing to paint its images if they are the
    /// same.
    stale: bool,
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
            stale: false,
        }
    }

    /// Applies one watch-stream message: a whole scene, a patch or problems.
    pub fn apply(&mut self, bytes: &[u8]) -> Applied {
        let Ok(update) = Update::decode(bytes) else {
            return Applied::Nothing;
        };
        let applied = match update.body {
            Some(Body::Scene(whole)) => {
                // Image files the canvas already holds aren't decoded again.
                let held = self.scene.take().map(|s| s.images).unwrap_or_default();
                wire::decode_scene(&whole, &held).map(|scene| self.scene = Some(scene))
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
    fn laid_out(&mut self) -> Option<&Drawing> {
        if self.stale || self.laid_out.is_none() {
            self.stale = false;
            let scene = self.scene.as_ref()?;
            let last = self.laid_out.take();
            self.laid_out = self.engine.drawing(scene, last.as_ref()).ok();
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
            })),
            ..Default::default()
        }
        .encode_to_vec();
        assert_eq!(design.apply(&patch), Applied::Changed);
        assert!(design.problems().is_empty());
        assert_eq!(design.size(), vec![300.0, 120.0]);
    }

    #[test]
    fn a_patch_before_any_scene_or_garbage_does_nothing() {
        let mut design = Design::new();
        let patch = Update {
            body: Some(Body::Patch(Patch {
                replace: vec![],
                images: vec![],
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
