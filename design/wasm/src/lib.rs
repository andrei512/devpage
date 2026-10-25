//! The design engine for the browser. Each canvas holds a `Design`: the scene
//! its watch stream built, laid out once per change, painted on the GPU by
//! `Gpu` or on the CPU by `Design::paint`, and hit-tested for clicks.

use devpage_design::proto::{Update, update::Body};
use devpage_design::render::{Draw, Frame};
use devpage_design::resolve::Node;
use devpage_design::{Renderer, Scene, wire};
use prost::Message;
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
mod gpu;

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
    laid_out: Option<LaidOut>,
}

/// A scene laid out and turned into its display list.
struct LaidOut {
    frames: Vec<Frame>,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    list: Vec<Draw>,
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
        }
    }

    /// Applies one watch-stream message: a whole scene, a patch or problems.
    pub fn apply(&mut self, bytes: &[u8]) -> Applied {
        let Ok(update) = Update::decode(bytes) else {
            return Applied::Nothing;
        };
        match update.body {
            Some(Body::Scene(root)) => {
                self.scene = Some(Scene {
                    root: wire::decode(&root),
                    files: Vec::new(),
                });
            }
            Some(Body::Patch(patch)) => match self.scene.as_mut() {
                Some(scene) => wire::apply(&mut scene.root, &patch.replace),
                None => return Applied::Nothing,
            },
            Some(Body::Problems(problems)) => {
                self.problems = problems.problems;
                return Applied::Problems;
            }
            None => return Applied::Nothing,
        }
        self.problems.clear();
        self.laid_out = None;
        Applied::Changed
    }

    /// The problems the last message reported, one per line, until a scene
    /// or a patch arrives.
    pub fn problems(&self) -> Vec<String> {
        self.problems.clone()
    }

    /// The design's size in its own pixels, `[width, height]`, or empty
    /// before the first scene.
    pub fn size(&mut self) -> Vec<f32> {
        match self.laid_out() {
            Some(l) => vec![l.frames[0].width, l.frames[0].height],
            None => Vec::new(),
        }
    }

    /// The interaction of the topmost node at `(x, y)`, in the design's
    /// pixels, that has one.
    pub fn action_at(&mut self, x: f32, y: f32) -> Option<String> {
        let root = self.scene.as_ref()?.root.clone();
        let frames = &self.laid_out()?.frames;
        let mut found = None;
        hit(&root, frames, &mut 0, x, y, &mut found);
        found
    }

    /// Paints the design on the CPU with `vello_cpu`, at `scale` device
    /// pixels per design pixel.
    pub fn paint(&mut self, scale: f32) -> Option<Pixels> {
        let scene = self.scene.as_ref()?;
        let pixmap = self.engine.paint(scene, scale);
        let mut rgba = pixmap.data_as_u8_slice().to_vec();
        for px in rgba.chunks_exact_mut(4) {
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
    fn laid_out(&mut self) -> Option<&LaidOut> {
        if self.laid_out.is_none() {
            let scene = self.scene.as_ref()?;
            let frames = self.engine.layout(scene);
            let (_, list) = self.engine.display_list(scene);
            self.laid_out = Some(LaidOut { frames, list });
        }
        self.laid_out.as_ref()
    }
}

/// Walks in paint order, so the last hit is the topmost.
fn hit(node: &Node, frames: &[Frame], at: &mut usize, x: f32, y: f32, found: &mut Option<String>) {
    let f = frames[*at];
    *at += 1;
    if let Some(action) = &node.on_click
        && x >= f.x
        && y >= f.y
        && x < f.x + f.width
        && y < f.y + f.height
    {
        *found = Some(action.clone());
    }
    for child in &node.children {
        hit(child, frames, at, x, y, found);
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
        ])
    }

    fn scene_message() -> Vec<u8> {
        let scene = devpage_design::resolve(&files(), "screen.design").unwrap();
        Update {
            body: Some(Body::Scene(wire::encode(&scene.root))),
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
            Some("navigate ./next.design")
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
            })),
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
            body: Some(Body::Patch(Patch { replace: vec![] })),
        }
        .encode_to_vec();
        assert_eq!(design.apply(&patch), Applied::Nothing);
        assert_eq!(design.apply(&[0xff, 0xff, 0xff]), Applied::Nothing);
        assert!(design.paint(1.0).is_none());
    }
}
