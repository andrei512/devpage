//! Scene → pixels: lays the scene out with taffy, sets text with cosmic-text
//! and paints with tiny-skia, on the CPU, the same way on every target.

use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, Weight};
use taffy::prelude::*;
use tiny_skia::{
    FillRule, Paint, PathBuilder, Pixmap, PixmapPaint, Rect as SkRect, Stroke, Transform,
};

use crate::resolve::{self, Align, Color, Kind, Layout, Node, Scene, Size as DSize};

const FONTS: [&[u8]; 4] = [
    include_bytes!("../fonts/Inter-Regular.ttf"),
    include_bytes!("../fonts/Inter-Medium.ttf"),
    include_bytes!("../fonts/Inter-SemiBold.ttf"),
    include_bytes!("../fonts/Inter-Bold.ttf"),
];

/// The fonts and glyph cache a renderer keeps between paints.
pub struct Renderer {
    fonts: FontSystem,
    glyphs: SwashCache,
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

impl Renderer {
    pub fn new() -> Renderer {
        let mut db = cosmic_text::fontdb::Database::new();
        for font in FONTS {
            db.load_font_data(font.to_vec());
        }
        Renderer {
            fonts: FontSystem::new_with_locale_and_db("en-US".into(), db),
            glyphs: SwashCache::new(),
        }
    }

    /// Lays the scene out: every node's box, in tree order.
    pub fn layout(&mut self, scene: &Scene) -> Vec<Frame> {
        let mut tree: TaffyTree<Option<TextKey>> = TaffyTree::new();
        let mut texts = Vec::new();
        let root = build(&mut tree, &scene.root, None, &mut texts, &mut self.fonts);
        let fonts = &mut self.fonts;
        tree.compute_layout_with_measure(root, Size::MAX_CONTENT, |input, _id, ctx, style| {
            let known = input.known_dimensions;
            let fixed = |d: Dimension| (!d.is_auto()).then(|| d.value());
            let known = Size {
                width: known.width.or(fixed(style.size.width)),
                height: known.height.or(fixed(style.size.height)),
            };
            let size = match ctx {
                Some(Some(key)) => {
                    let width = known.width.or(match input.available_space.width {
                        AvailableSpace::Definite(w) if texts[key.0].1 => Some(w),
                        _ => None,
                    });
                    let buffer = &mut texts[key.0].0;
                    buffer.set_size(width, None);
                    buffer.shape_until_scroll(fonts, false);
                    let (w, h) = measure(buffer);
                    Size {
                        width: known.width.unwrap_or(w),
                        height: known.height.unwrap_or(h),
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

    /// Paints the scene at `scale` device pixels per scene pixel.
    pub fn paint(&mut self, scene: &Scene, scale: f32) -> Pixmap {
        let frames = self.layout(scene);
        let root = frames[0];
        let w = ((root.width * scale).ceil() as u32).max(1);
        let h = ((root.height * scale).ceil() as u32).max(1);
        let mut pixmap = Pixmap::new(w, h).expect("a non-empty canvas");
        let mut at = 0;
        self.node(&mut pixmap, &scene.root, &frames, &mut at, scale, 1.0, None);
        pixmap
    }

    #[allow(clippy::too_many_arguments)]
    fn node(
        &mut self,
        pixmap: &mut Pixmap,
        node: &Node,
        frames: &[Frame],
        at: &mut usize,
        scale: f32,
        opacity: f32,
        clip: Option<&tiny_skia::Mask>,
    ) {
        let f = frames[*at];
        *at += 1;
        let opacity = opacity * node.opacity;
        let t = Transform::from_scale(scale, scale);
        let shape = |inset: f32| -> Option<tiny_skia::Path> {
            let r = SkRect::from_xywh(
                f.x + inset,
                f.y + inset,
                f.width - 2.0 * inset,
                f.height - 2.0 * inset,
            )?;
            if node.kind == Kind::Ellipse {
                PathBuilder::from_oval(r)
            } else {
                rounded(r, node.radius.map(|v| (v - inset).max(0.0)))
            }
        };
        if let Some(s) = &node.shadow
            && let Some(path) = shape(-s.spread)
        {
            shadow(pixmap, &path, s, scale, opacity, clip);
        }
        if let (Some(c), Some(path)) = (node.fill, shape(0.0)) {
            pixmap.fill_path(&path, &paint(c, opacity), FillRule::Winding, t, clip);
        }
        if let (Some(c), Some(path)) = (node.stroke, shape(node.stroke_width / 2.0)) {
            let stroke = Stroke {
                width: node.stroke_width,
                ..Stroke::default()
            };
            pixmap.stroke_path(&path, &paint(c, opacity), &stroke, t, clip);
        }
        if let Some(text) = &node.text {
            self.text(pixmap, text, f, scale, opacity, clip);
        }
        let mut own_clip = None;
        if node.clip
            && let Some(path) = shape(0.0)
        {
            let mut mask = match clip {
                Some(m) => m.clone(),
                None => {
                    let mut m =
                        tiny_skia::Mask::new(pixmap.width(), pixmap.height()).expect("mask");
                    m.fill_path(
                        &PathBuilder::from_rect(
                            SkRect::from_xywh(
                                0.0,
                                0.0,
                                pixmap.width() as f32,
                                pixmap.height() as f32,
                            )
                            .expect("rect"),
                        ),
                        FillRule::Winding,
                        false,
                        Transform::identity(),
                    );
                    m
                }
            };
            mask.intersect_path(&path, FillRule::Winding, true, t);
            own_clip = Some(mask);
        }
        let clip = own_clip.as_ref().or(clip);
        for child in &node.children {
            self.node(pixmap, child, frames, at, scale, opacity, clip);
        }
    }

    fn text(
        &mut self,
        pixmap: &mut Pixmap,
        text: &resolve::TextStyle,
        f: Frame,
        scale: f32,
        opacity: f32,
        clip: Option<&tiny_skia::Mask>,
    ) {
        let mut buffer = text_buffer(&mut self.fonts, text, scale);
        buffer.set_size(Some(f.width * scale + 1.0), None);
        buffer.shape_until_scroll(&mut self.fonts, false);
        let c = text.color;
        let color = cosmic_text::Color::rgba(c.r, c.g, c.b, c.a);
        let (ox, oy) = ((f.x * scale).round() as i32, (f.y * scale).round() as i32);
        let mut glyphs = Pixmap::new(pixmap.width(), pixmap.height()).expect("layer");
        let data = glyphs.data_mut();
        let (pw, ph) = (pixmap.width() as i32, pixmap.height() as i32);
        buffer.draw(
            &mut self.fonts,
            &mut self.glyphs,
            color,
            |x, y, w, h, color| {
                for yy in 0..h as i32 {
                    for xx in 0..w as i32 {
                        let (px, py) = (ox + x + xx, oy + y + yy);
                        if px < 0 || py < 0 || px >= pw || py >= ph {
                            continue;
                        }
                        let a = color.a() as u32;
                        let i = ((py * pw + px) * 4) as usize;
                        // Premultiplied, source-over onto the layer.
                        let src = [
                            color.r() as u32 * a / 255,
                            color.g() as u32 * a / 255,
                            color.b() as u32 * a / 255,
                            a,
                        ];
                        for k in 0..4 {
                            let dst = data[i + k] as u32;
                            data[i + k] = (src[k] + dst * (255 - a) / 255) as u8;
                        }
                    }
                }
            },
        );
        let paint = PixmapPaint {
            opacity,
            ..PixmapPaint::default()
        };
        pixmap.draw_pixmap(0, 0, glyphs.as_ref(), &paint, Transform::identity(), clip);
    }
}

struct TextKey(usize);

fn text_buffer(fonts: &mut FontSystem, text: &resolve::TextStyle, scale: f32) -> Buffer {
    let metrics = Metrics::new(text.size * scale, text.size * text.line_height * scale);
    let mut buffer = Buffer::new(fonts, metrics);
    let attrs = Attrs::new()
        .family(Family::Name("Inter"))
        .weight(Weight(text.weight));
    buffer.set_text(&text.content, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(fonts, false);
    buffer
}

fn measure(buffer: &Buffer) -> (f32, f32) {
    let mut w: f32 = 0.0;
    let mut h: f32 = 0.0;
    for run in buffer.layout_runs() {
        w = w.max(run.line_w);
        h = h.max(run.line_top + run.line_height);
    }
    (w.ceil(), h)
}

fn build(
    tree: &mut TaffyTree<Option<TextKey>>,
    node: &Node,
    parent: Option<&Node>,
    texts: &mut Vec<(Buffer, bool)>,
    fonts: &mut FontSystem,
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
        texts.push((text_buffer(fonts, text, 1.0), wraps));
        return tree
            .new_leaf_with_context(style, Some(TextKey(texts.len() - 1)))
            .expect("leaf");
    }
    let children: Vec<NodeId> = node
        .children
        .iter()
        .map(|c| build(tree, c, Some(node), texts, fonts))
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

fn paint(c: Color, opacity: f32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c.r, c.g, c.b, (c.a as f32 * opacity).round() as u8);
    p.anti_alias = true;
    p
}

fn rounded(r: SkRect, radius: [f32; 4]) -> Option<tiny_skia::Path> {
    let max = r.width().min(r.height()) / 2.0;
    let [tl, tr, br, bl] = radius.map(|v| v.min(max));
    if tl + tr + br + bl == 0.0 {
        return Some(PathBuilder::from_rect(r));
    }
    // A circular arc as a cubic: the control points sit k·radius along the tangents.
    let k = 0.552_284_8;
    let (x0, y0, x1, y1) = (r.left(), r.top(), r.right(), r.bottom());
    let mut pb = PathBuilder::new();
    pb.move_to(x0 + tl, y0);
    pb.line_to(x1 - tr, y0);
    pb.cubic_to(x1 - tr + tr * k, y0, x1, y0 + tr - tr * k, x1, y0 + tr);
    pb.line_to(x1, y1 - br);
    pb.cubic_to(x1, y1 - br + br * k, x1 - br + br * k, y1, x1 - br, y1);
    pb.line_to(x0 + bl, y1);
    pb.cubic_to(x0 + bl - bl * k, y1, x0, y1 - bl + bl * k, x0, y1 - bl);
    pb.line_to(x0, y0 + tl);
    pb.cubic_to(x0, y0 + tl - tl * k, x0 + tl - tl * k, y0, x0 + tl, y0);
    pb.close();
    pb.finish()
}

/// A blurred shadow: the shape drawn in the shadow's color on a layer, box
/// blurred three times — close to a Gaussian — and drawn under the node.
fn shadow(
    pixmap: &mut Pixmap,
    path: &tiny_skia::Path,
    s: &resolve::Shadow,
    scale: f32,
    opacity: f32,
    clip: Option<&tiny_skia::Mask>,
) {
    let mut layer = Pixmap::new(pixmap.width(), pixmap.height()).expect("layer");
    let t = Transform::from_translate(s.x, s.y).post_scale(scale, scale);
    layer.fill_path(path, &paint(s.color, opacity), FillRule::Winding, t, None);
    let radius = (s.blur * scale / 2.0).round() as usize;
    if radius > 0 {
        for _ in 0..3 {
            box_blur(&mut layer, radius / 2 + 1);
        }
    }
    pixmap.draw_pixmap(
        0,
        0,
        layer.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        clip,
    );
}

fn box_blur(p: &mut Pixmap, r: usize) {
    let (w, h) = (p.width() as usize, p.height() as usize);
    let data = p.data_mut();
    let mut tmp = vec![0u8; data.len()];
    for pass in 0..2 {
        let (src, dst): (&[u8], &mut [u8]) = if pass == 0 {
            (&*data, &mut tmp)
        } else {
            (&tmp, &mut *data)
        };
        let src = src.to_vec();
        let (outer, inner) = if pass == 0 { (h, w) } else { (w, h) };
        for o in 0..outer {
            for c in 0..4 {
                let at = |i: usize| {
                    if pass == 0 {
                        (o * w + i) * 4 + c
                    } else {
                        (i * w + o) * 4 + c
                    }
                };
                let mut sum: u32 = 0;
                for i in 0..r.min(inner) {
                    sum += src[at(i)] as u32;
                }
                let n = (2 * r + 1) as u32;
                for i in 0..inner {
                    if i + r < inner {
                        sum += src[at(i + r)] as u32;
                    }
                    dst[at(i)] = (sum / n) as u8;
                    if i >= r {
                        sum -= src[at(i - r)] as u32;
                    }
                }
            }
        }
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
        let px = pixmap.pixel(90, 30).unwrap();
        assert_eq!(
            (px.red(), px.green(), px.blue(), px.alpha()),
            (255, 0, 0, 255)
        );
        // Some pixel of the text is lighter than the fill.
        assert!((8..40).any(|x| (8..28).any(|y| pixmap.pixel(x, y).unwrap().green() > 128)));
    }
}
