//! Scene → pixels. The engine lays a scene out once (taffy, parley) and
//! turns it into a display list of vello drawing commands; a backend draws
//! the list — `vello_cpu` here, for the PNG.

use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontWeight, Layout as Text,
    LayoutContext, LineHeight, PositionedLayoutItem, StyleProperty,
};
use taffy::prelude::*;
use vello_cpu::kurbo::{Affine, BezPath, Ellipse as KEllipse, Rect as KRect, RoundedRect, Shape};
use vello_cpu::peniko::{Color as PColor, FontData};
use vello_cpu::{Glyph, Pixmap, RenderContext, Resources};

use crate::resolve::{self, Align, Color, Kind, Layout, Node, Scene, Size as DSize};

const FONTS: [&[u8]; 4] = [
    include_bytes!("../fonts/Inter-Regular.ttf"),
    include_bytes!("../fonts/Inter-Medium.ttf"),
    include_bytes!("../fonts/Inter-SemiBold.ttf"),
    include_bytes!("../fonts/Inter-Bold.ttf"),
];

/// The fonts and text layout state a renderer keeps between paints.
pub struct Renderer {
    fonts: FontContext,
    layouts: LayoutContext<()>,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

/// A node's box after layout, in the scene's pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// One drawing command, in scene pixels: what both vello backends replay.
pub enum Draw {
    Fill(BezPath, PColor),
    Stroke(BezPath, f32, PColor),
    Shadow(KRect, f32, f32, PColor),
    Glyphs(FontData, f32, PColor, Vec<Glyph>),
    PushClip(BezPath),
    PushOpacity(f32),
    Pop,
}

impl Renderer {
    pub fn new() -> Renderer {
        let mut fonts = FontContext {
            collection: parley::fontique::Collection::new(parley::fontique::CollectionOptions {
                shared: false,
                system_fonts: false,
            }),
            source_cache: parley::fontique::SourceCache::default(),
        };
        for font in FONTS {
            fonts
                .collection
                .register_fonts(parley::fontique::Blob::new(std::sync::Arc::new(font)), None);
        }
        Renderer {
            fonts,
            layouts: LayoutContext::new(),
        }
    }

    fn text_layout(&mut self, text: &resolve::TextStyle, width: Option<f32>) -> Text<()> {
        let mut builder = self
            .layouts
            .ranged_builder(&mut self.fonts, &text.content, 1.0, true);
        builder.push_default(StyleProperty::FontFamily(FontFamily::named("Inter")));
        builder.push_default(StyleProperty::FontSize(text.size));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(
            text.weight as f32,
        )));
        builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
            text.line_height,
        )));
        let mut layout = builder.build(&text.content);
        layout.break_all_lines(width);
        layout.align(Alignment::Start, AlignmentOptions::default());
        layout
    }

    /// Lays the scene out: every node's box, in tree order.
    pub fn layout(&mut self, scene: &Scene) -> Vec<Frame> {
        let mut tree: TaffyTree<Option<TextKey>> = TaffyTree::new();
        let mut texts = Vec::new();
        let root = build(&mut tree, &scene.root, None, &mut texts);
        tree.compute_layout_with_measure(root, Size::MAX_CONTENT, |input, _id, ctx, style| {
            let known = input.known_dimensions;
            let fixed = |d: Dimension| (!d.is_auto()).then(|| d.value());
            let known = Size {
                width: known.width.or(fixed(style.size.width)),
                height: known.height.or(fixed(style.size.height)),
            };
            let size = match ctx {
                Some(Some(key)) => {
                    let (text, wraps): &(resolve::TextStyle, bool) = &texts[key.0];
                    let width = known.width.or(match input.available_space.width {
                        AvailableSpace::Definite(w) if *wraps => Some(w),
                        _ => None,
                    });
                    let layout = self.text_layout(text, width);
                    Size {
                        width: known.width.unwrap_or(layout.width().ceil()),
                        height: known.height.unwrap_or(layout.height()),
                    }
                }
                _ => Size {
                    width: known.width.unwrap_or(0.0),
                    height: known.height.unwrap_or(0.0),
                },
            };
            taffy::LayoutOutput::from_outer_size(size)
        })
        .expect("layout");
        let mut frames = Vec::new();
        collect(&tree, root, 0.0, 0.0, &mut frames);
        frames
    }

    /// The scene as drawing commands: laid out once, shaped once.
    pub fn display_list(&mut self, scene: &Scene) -> (Frame, Vec<Draw>) {
        let frames = self.layout(scene);
        let mut list = Vec::new();
        let mut at = 0;
        self.node(&scene.root, &frames, &mut at, &mut list);
        (frames[0], list)
    }

    /// Paints the scene with `vello_cpu` at `scale` device pixels per scene pixel.
    pub fn paint(&mut self, scene: &Scene, scale: f32) -> Pixmap {
        let (root, list) = self.display_list(scene);
        let w = ((root.width * scale).ceil() as u16).max(1);
        let h = ((root.height * scale).ceil() as u16).max(1);
        let mut ctx = RenderContext::new(w, h);
        let mut resources = Resources::default();
        ctx.set_transform(Affine::scale(scale as f64));
        for draw in &list {
            match draw {
                Draw::Fill(path, c) => {
                    ctx.set_paint(*c);
                    ctx.fill_path(path);
                }
                Draw::Stroke(path, width, c) => {
                    ctx.set_paint(*c);
                    ctx.set_stroke(vello_cpu::kurbo::Stroke::new(*width as f64));
                    ctx.stroke_path(path);
                }
                Draw::Shadow(rect, radius, std_dev, c) => {
                    ctx.set_paint(*c);
                    ctx.fill_blurred_rounded_rect(rect, *radius, *std_dev, false);
                }
                Draw::Glyphs(font, size, c, glyphs) => {
                    ctx.set_paint(*c);
                    let _ = ctx
                        .glyph_run(&mut resources, font)
                        .font_size(*size)
                        .hint(true)
                        .fill_glyphs(glyphs.iter().copied());
                }
                Draw::PushClip(path) => ctx.push_clip_layer(path),
                Draw::PushOpacity(o) => ctx.push_opacity_layer(*o),
                Draw::Pop => ctx.pop_layer(),
            }
        }
        ctx.flush();
        let mut pixmap = Pixmap::new(w, h);
        ctx.render(&mut pixmap, &mut resources);
        pixmap
    }

    fn node(&mut self, node: &Node, frames: &[Frame], at: &mut usize, list: &mut Vec<Draw>) {
        let f = frames[*at];
        *at += 1;
        let opacity = node.opacity < 1.0;
        if opacity {
            list.push(Draw::PushOpacity(node.opacity));
        }
        let rect = KRect::new(
            f.x as f64,
            f.y as f64,
            (f.x + f.width) as f64,
            (f.y + f.height) as f64,
        );
        let shape = |inset: f64| -> BezPath {
            let r = rect.inset(-inset);
            if node.kind == Kind::Ellipse {
                KEllipse::from_rect(r).to_path(0.1)
            } else {
                let [tl, tr, br, bl] = node.radius.map(|v| (v as f64 - inset).max(0.0));
                RoundedRect::from_rect(r, (tl, tr, br, bl)).to_path(0.1)
            }
        };
        if let Some(s) = &node.shadow {
            let r = rect.inflate(s.spread as f64, s.spread as f64)
                + vello_cpu::kurbo::Vec2::new(s.x as f64, s.y as f64);
            list.push(Draw::Shadow(
                r,
                node.radius[0],
                s.blur / 2.0,
                color(s.color),
            ));
        }
        if let Some(c) = node.fill {
            list.push(Draw::Fill(shape(0.0), color(c)));
        }
        if let Some(c) = node.stroke {
            list.push(Draw::Stroke(
                shape(node.stroke_width as f64 / 2.0),
                node.stroke_width,
                color(c),
            ));
        }
        if let Some(text) = &node.text {
            let layout = self.text_layout(text, Some(f.width + 0.5));
            for line in layout.lines() {
                for item in line.items() {
                    if let PositionedLayoutItem::GlyphRun(run) = item {
                        let glyphs = run
                            .positioned_glyphs()
                            .map(|g| Glyph {
                                id: g.id,
                                x: f.x + g.x,
                                y: f.y + g.y,
                            })
                            .collect();
                        list.push(Draw::Glyphs(
                            run.run().font().clone(),
                            run.run().font_size(),
                            color(text.color),
                            glyphs,
                        ));
                    }
                }
            }
        }
        if node.clip {
            list.push(Draw::PushClip(shape(0.0)));
        }
        for child in &node.children {
            self.node(child, frames, at, list);
        }
        if node.clip {
            list.push(Draw::Pop);
        }
        if opacity {
            list.push(Draw::Pop);
        }
    }
}

struct TextKey(usize);

fn color(c: Color) -> PColor {
    PColor::from_rgba8(c.r, c.g, c.b, c.a)
}

fn build(
    tree: &mut TaffyTree<Option<TextKey>>,
    node: &Node,
    parent: Option<&Node>,
    texts: &mut Vec<(resolve::TextStyle, bool)>,
) -> NodeId {
    let mut style = Style::default();
    let dir = parent.map(|p| p.layout);
    let dim = |s: DSize| match s {
        DSize::Px(v) => Dimension::length(v),
        _ => Dimension::auto(),
    };
    style.size = Size {
        width: dim(node.width),
        height: dim(node.height),
    };
    let (main, cross) = match dir {
        Some(Layout::Row) => (node.width, node.height),
        Some(Layout::Column) => (node.height, node.width),
        _ => (DSize::Hug, DSize::Hug),
    };
    if main == DSize::Fill {
        style.flex_grow = 1.0;
        style.flex_basis = Dimension::length(0.0);
    }
    if cross == DSize::Fill {
        style.align_self = Some(AlignSelf::STRETCH);
    }
    if dir == Some(Layout::None) {
        style.position = Position::Absolute;
        style.inset = taffy::Rect {
            left: LengthPercentageAuto::length(node.x),
            top: LengthPercentageAuto::length(node.y),
            right: LengthPercentageAuto::auto(),
            bottom: LengthPercentageAuto::auto(),
        };
    }
    style.flex_shrink = 0.0;
    match node.layout {
        Layout::Row => style.flex_direction = FlexDirection::Row,
        Layout::Column => style.flex_direction = FlexDirection::Column,
        Layout::None => {}
    }
    style.gap = Size {
        width: LengthPercentage::length(node.gap),
        height: LengthPercentage::length(node.gap),
    };
    let [t, r, b, l] = node.padding;
    style.padding = taffy::Rect {
        top: LengthPercentage::length(t),
        right: LengthPercentage::length(r),
        bottom: LengthPercentage::length(b),
        left: LengthPercentage::length(l),
    };
    style.align_items = match node.align {
        Align::Center => AlignItems::CENTER,
        Align::End => AlignItems::FLEX_END,
        _ => AlignItems::FLEX_START,
    };
    style.justify_content = match node.justify {
        Align::Center => JustifyContent::CENTER,
        Align::End => JustifyContent::FLEX_END,
        Align::SpaceBetween => JustifyContent::SPACE_BETWEEN,
        Align::Start => JustifyContent::FLEX_START,
    };
    if let Some(text) = &node.text {
        let wraps = !matches!(node.width, DSize::Hug);
        texts.push((text.clone(), wraps));
        return tree
            .new_leaf_with_context(style, Some(TextKey(texts.len() - 1)))
            .expect("leaf");
    }
    let children: Vec<NodeId> = node
        .children
        .iter()
        .map(|c| build(tree, c, Some(node), texts))
        .collect();
    tree.new_with_children(style, &children).expect("node")
}

fn collect(tree: &TaffyTree<Option<TextKey>>, id: NodeId, ox: f32, oy: f32, out: &mut Vec<Frame>) {
    let l = tree.layout(id).expect("laid out");
    let (x, y) = (ox + l.location.x, oy + l.location.y);
    out.push(Frame {
        x,
        y,
        width: l.size.width,
        height: l.size.height,
    });
    for child in tree.children(id).expect("children") {
        collect(tree, child, x, y, out);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn scene(source: &str) -> Scene {
        let files: HashMap<String, String> = [("a.design".to_string(), source.to_string())].into();
        resolve::resolve(&files, "a.design").unwrap()
    }

    #[test]
    fn auto_layout_places_children() {
        let s = scene(
            r#"<Frame width={200} height={100} layout="row" gap={10} padding={20}>
  <Rect width={30} height={30} />
  <Rect width="fill" height="fill" />
</Frame>"#,
        );
        let frames = Renderer::new().layout(&s);
        assert_eq!(
            frames[1],
            Frame {
                x: 20.0,
                y: 20.0,
                width: 30.0,
                height: 30.0
            }
        );
        assert_eq!(
            frames[2],
            Frame {
                x: 60.0,
                y: 20.0,
                width: 120.0,
                height: 60.0
            }
        );
    }

    #[test]
    fn text_hugs_its_content() {
        let s =
            scene(r#"<Frame layout="column" padding={10}><Text size={20}>Hello</Text></Frame>"#);
        let frames = Renderer::new().layout(&s);
        assert!(
            frames[1].width > 40.0 && frames[1].width < 60.0,
            "{:?}",
            frames[1]
        );
        assert_eq!(frames[1].height, 28.0);
        assert_eq!(frames[0].height, 48.0);
    }

    #[test]
    fn paints_fills_and_text() {
        let s = scene(
            r##"<Frame width={100} height={40} fill="#ff0000" layout="row" padding={8}><Text color="#ffffff">Pay</Text></Frame>"##,
        );
        let pixmap = Renderer::new().paint(&s, 1.0);
        assert_eq!((pixmap.width(), pixmap.height()), (100, 40));
        let p = pixmap.sample(90, 30);
        assert_eq!((p.r, p.g, p.b, p.a), (255, 0, 0, 255));
        // Some pixel of the text is lighter than the fill.
        assert!((8..40).any(|x| (8..28).any(|y| pixmap.sample(x, y).g > 128)));
    }
}
