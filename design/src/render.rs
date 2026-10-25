//! Scene → pixels. The engine lays a scene out once (taffy, parley) and
//! turns it into a display list of vello drawing commands; a backend draws
//! the list — `vello_cpu` here, for the PNG.

use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontWeight, Layout as Text,
    LayoutContext, LineHeight, PositionedLayoutItem, StyleProperty,
};
use std::collections::HashMap;
use std::sync::Arc;
use taffy::prelude::*;

use vello_cpu::kurbo::{
    Affine, BezPath, Ellipse as KEllipse, Rect as KRect, RoundedRect, Shape, Vec2,
};
use vello_cpu::peniko::{
    Blob, Color as PColor, FontData, Gradient, ImageAlphaType, ImageBrush, ImageData, ImageFormat,
};
use vello_cpu::{Glyph, Pixmap, RenderContext, Resources};

use crate::resolve::{
    self, Align, Color, Fill, Kind, Layout, Node, Scene, Size as DSize, TextAlign,
};

const FONTS: [&[u8]; 4] = [
    include_bytes!("../fonts/Inter-Regular.ttf"),
    include_bytes!("../fonts/Inter-Medium.ttf"),
    include_bytes!("../fonts/Inter-SemiBold.ttf"),
    include_bytes!("../fonts/Inter-Bold.ttf"),
];

/// The fonts, text layout state and decoded images a renderer keeps between
/// paints.
pub struct Renderer {
    fonts: FontContext,
    layouts: LayoutContext<()>,
    images: HashMap<String, (Vec<u8>, Option<ImageData>)>,
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

/// What a `Draw::Fill` covers its path with.
pub enum Paint {
    Solid(PColor),
    /// A gradient whose points are in scene pixels.
    Gradient(Gradient),
    /// An image, placed by the transform from its pixels to scene pixels.
    Image(ImageData, Affine),
}

/// One drawing command, in scene pixels: what both vello backends replay.
/// Every decision about the drawing is made before a command is made; a
/// backend only translates it into its library's call.
pub enum Draw {
    Fill(BezPath, Paint),
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
            images: HashMap::new(),
        }
    }

    /// An image's pixels, decoded once per file contents.
    fn image(&mut self, image: &resolve::Image) -> Option<ImageData> {
        if let Some((data, decoded)) = self.images.get(&image.src)
            && *data == image.data
        {
            return decoded.clone();
        }
        let decoded = decode(&image.data);
        self.images
            .insert(image.src.clone(), (image.data.clone(), decoded.clone()));
        decoded
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
        let align = match text.align {
            TextAlign::Left => Alignment::Left,
            TextAlign::Center => Alignment::Center,
            TextAlign::Right => Alignment::Right,
        };
        layout.align(align, AlignmentOptions::default());
        layout
    }

    /// Lays the scene out: every node's box, in tree order.
    pub fn layout(&mut self, scene: &Scene) -> Vec<Frame> {
        let mut tree: TaffyTree<Option<Leaf>> = TaffyTree::new();
        let mut texts = Vec::new();
        let root = build(&mut tree, &scene.root, None, &mut texts, self);
        tree.compute_layout_with_measure(root, Size::MAX_CONTENT, |input, _id, ctx, style| {
            let known = input.known_dimensions;
            let fixed = |d: Dimension| (!d.is_auto()).then(|| d.value());
            let known = Size {
                width: known.width.or(fixed(style.size.width)),
                height: known.height.or(fixed(style.size.height)),
            };
            let size = match ctx {
                Some(Some(Leaf::Intrinsic(w, h))) => {
                    // A vector or an image hugging keeps its own aspect.
                    let (w, h) = (*w, *h);
                    match (known.width, known.height) {
                        (Some(kw), None) => Size {
                            width: kw,
                            height: kw * h / w,
                        },
                        (None, Some(kh)) => Size {
                            width: kh * w / h,
                            height: kh,
                        },
                        (kw, kh) => Size {
                            width: kw.unwrap_or(w),
                            height: kh.unwrap_or(h),
                        },
                    }
                }
                Some(Some(Leaf::Text(key))) => {
                    let (text, wraps): &(resolve::TextStyle, bool) = &texts[*key];
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
                Draw::Fill(path, paint) => {
                    match paint {
                        Paint::Solid(c) => ctx.set_paint(*c),
                        Paint::Gradient(g) => ctx.set_paint(g.clone()),
                        Paint::Image(data, place) => {
                            ctx.set_paint(ImageBrush {
                                image: vello_cpu::ImageSource::from_peniko_image_data(data),
                                sampler: Default::default(),
                            });
                            ctx.set_paint_transform(*place);
                        }
                    }
                    ctx.fill_path(path);
                    ctx.reset_paint_transform();
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
            let r =
                rect.inflate(s.spread as f64, s.spread as f64) + Vec2::new(s.x as f64, s.y as f64);
            let radius = if node.kind == Kind::Ellipse {
                (f.width.min(f.height) / 2.0) + s.spread
            } else {
                node.radius[0] + s.spread
            };
            list.push(Draw::Shadow(
                r,
                radius.max(0.0),
                s.blur / 2.0,
                color(s.color),
            ));
        }
        // A vector's fill and stroke follow its path; every other box's
        // follow the box.
        let vector = node.vector.as_ref().map(|v| {
            let [vx, vy, vw, vh] = v.view_box.map(|n| n as f64);
            let s = (f.width as f64 / vw).min(f.height as f64 / vh);
            let tx = f.x as f64 + (f.width as f64 - vw * s) / 2.0 - vx * s;
            let ty = f.y as f64 + (f.height as f64 - vh * s) / 2.0 - vy * s;
            let mut path = BezPath::from_svg(&v.d).unwrap_or_default();
            path.apply_affine(Affine::translate((tx, ty)) * Affine::scale(s));
            path
        });
        if let Some(fill) = &node.fill {
            let paint = match fill {
                Fill::Solid(c) => Paint::Solid(color(*c)),
                Fill::Linear { angle, stops } => Paint::Gradient(gradient(rect, *angle, stops)),
            };
            let path = vector.clone().unwrap_or_else(|| shape(0.0));
            list.push(Draw::Fill(path, paint));
        }
        if let Some(image) = &node.image
            && let Some(data) = self.image(image)
        {
            // Covering the box: scaled to fill it, centered, cut to its shape.
            let (iw, ih) = (data.width as f64, data.height as f64);
            let s = (f.width as f64 / iw).max(f.height as f64 / ih);
            let place = Affine::translate((
                f.x as f64 + (f.width as f64 - iw * s) / 2.0,
                f.y as f64 + (f.height as f64 - ih * s) / 2.0,
            )) * Affine::scale(s);
            list.push(Draw::Fill(shape(0.0), Paint::Image(data, place)));
        }
        if let Some(c) = node.stroke {
            let path = match &vector {
                Some(path) => path.clone(),
                None => shape(node.stroke_width as f64 / 2.0),
            };
            list.push(Draw::Stroke(path, node.stroke_width, color(c)));
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

/// What a leaf of the layout tree measures by: a text, or a vector's or an
/// image's own size.
enum Leaf {
    Text(usize),
    Intrinsic(f32, f32),
}

fn color(c: Color) -> PColor {
    PColor::from_rgba8(c.r, c.g, c.b, c.a)
}

/// A CSS linear gradient over `rect`: 0° points up, 90° right; the line runs
/// through the center, long enough that its ends touch the far corners.
fn gradient(rect: KRect, angle: f32, stops: &[(Color, f32)]) -> Gradient {
    let a = (angle as f64).to_radians();
    let dir = Vec2::new(a.sin(), -a.cos());
    let half = (rect.width() * a.sin().abs() + rect.height() * a.cos().abs()) / 2.0;
    let center = rect.center();
    let stops: Vec<(f32, PColor)> = stops.iter().map(|(c, at)| (*at, color(*c))).collect();
    Gradient::new_linear(center - dir * half, center + dir * half).with_stops(stops.as_slice())
}

/// A PNG's or JPEG's pixels, straight RGBA.
fn decode(data: &[u8]) -> Option<ImageData> {
    let (pixels, width, height) = if data.starts_with(b"\x89PNG") {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(data));
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        let mut reader = decoder.read_info().ok()?;
        let mut buf = vec![0; reader.output_buffer_size()?];
        let info = reader.next_frame(&mut buf).ok()?;
        buf.truncate(info.buffer_size());
        let rgba = match info.color_type {
            png::ColorType::Rgba => buf,
            png::ColorType::Rgb => buf
                .chunks(3)
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
            png::ColorType::GrayscaleAlpha => buf
                .chunks(2)
                .flat_map(|p| [p[0], p[0], p[0], p[1]])
                .collect(),
            png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
            png::ColorType::Indexed => return None,
        };
        (rgba, info.width, info.height)
    } else {
        use zune_jpeg::JpegDecoder;
        use zune_jpeg::zune_core::colorspace::ColorSpace;
        use zune_jpeg::zune_core::options::DecoderOptions;
        let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
        let mut decoder = JpegDecoder::new_with_options(std::io::Cursor::new(data), options);
        let pixels = decoder.decode().ok()?;
        let info = decoder.info()?;
        (pixels, info.width as u32, info.height as u32)
    };
    Some(ImageData {
        data: Blob::new(Arc::new(pixels)),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width,
        height,
    })
}

/// How far apart two drawings of the same scene may be — two rasterizers
/// differ only in the anti-aliasing of edges: at most this share of pixels may
/// have a channel more than [`EDGE`] apart.
pub const TOLERANCE: f64 = 0.01;
/// The largest per-channel difference that is not counted against
/// [`TOLERANCE`].
pub const EDGE: u8 = 8;

/// Whether two RGBA buffers of the same size are the same drawing within
/// [`TOLERANCE`]; the share of pixels that differ is the error.
pub fn matches(a: &[u8], b: &[u8]) -> Result<(), f64> {
    if a.len() != b.len() {
        return Err(1.0);
    }
    let differ = a
        .chunks(4)
        .zip(b.chunks(4))
        .filter(|(p, q)| p.iter().zip(*q).any(|(x, y)| x.abs_diff(*y) > EDGE))
        .count();
    let share = differ as f64 / (a.len() / 4).max(1) as f64;
    if share <= TOLERANCE {
        Ok(())
    } else {
        Err(share)
    }
}

fn build(
    tree: &mut TaffyTree<Option<Leaf>>,
    node: &Node,
    parent: Option<&Node>,
    texts: &mut Vec<(resolve::TextStyle, bool)>,
    renderer: &mut Renderer,
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
            .new_leaf_with_context(style, Some(Leaf::Text(texts.len() - 1)))
            .expect("leaf");
    }
    let intrinsic = match (&node.vector, &node.image) {
        (Some(v), _) => Some((v.view_box[2], v.view_box[3])),
        (_, Some(i)) => renderer.image(i).map(|d| (d.width as f32, d.height as f32)),
        _ => None,
    };
    if let Some((w, h)) = intrinsic {
        return tree
            .new_leaf_with_context(style, Some(Leaf::Intrinsic(w, h)))
            .expect("leaf");
    }
    let children: Vec<NodeId> = node
        .children
        .iter()
        .map(|c| build(tree, c, Some(node), texts, renderer))
        .collect();
    tree.new_with_children(style, &children).expect("node")
}

fn collect(tree: &TaffyTree<Option<Leaf>>, id: NodeId, ox: f32, oy: f32, out: &mut Vec<Frame>) {
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

    fn frames(source: &str) -> Vec<Frame> {
        Renderer::new().layout(&scene(source))
    }

    fn at(x: f32, y: f32, width: f32, height: f32) -> Frame {
        Frame {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn hug_takes_the_content_and_padding() {
        let f = frames(
            r#"<Frame layout="row" gap={4} padding={[2, 6]}>
  <Rect width={10} height={20} /><Rect width={10} height={8} />
</Frame>"#,
        );
        assert_eq!(f[0], at(0.0, 0.0, 36.0, 24.0));
        assert_eq!(f[1], at(6.0, 2.0, 10.0, 20.0));
        assert_eq!(f[2], at(20.0, 2.0, 10.0, 8.0));
    }

    #[test]
    fn fill_shares_the_free_space_and_stretches_across() {
        let f = frames(
            r#"<Frame width={100} height={50} layout="column" padding={[5, 10, 15, 20]}>
  <Rect height={10} width="fill" /><Rect height="fill" width={10} /><Rect height="fill" width={10} />
</Frame>"#,
        );
        assert_eq!(f[1], at(20.0, 5.0, 70.0, 10.0));
        assert_eq!(f[2], at(20.0, 15.0, 10.0, 10.0));
        assert_eq!(f[3], at(20.0, 25.0, 10.0, 10.0));
    }

    #[test]
    fn align_and_justify_place_children() {
        let row = |align: &str, justify: &str| {
            frames(&format!(
                r#"<Frame width={{100}} height={{40}} layout="row" align="{align}" justify="{justify}">
  <Rect width={{10}} height={{10}} /><Rect width={{20}} height={{20}} />
</Frame>"#
            ))
        };
        let f = row("center", "center");
        assert_eq!((f[1].x, f[1].y, f[2].x, f[2].y), (35.0, 15.0, 45.0, 10.0));
        let f = row("end", "end");
        assert_eq!((f[1].x, f[1].y, f[2].x, f[2].y), (70.0, 30.0, 80.0, 20.0));
        let f = row("start", "space-between");
        assert_eq!((f[1].x, f[1].y, f[2].x, f[2].y), (0.0, 0.0, 80.0, 0.0));
    }

    #[test]
    fn no_layout_places_children_at_their_position() {
        let f = frames(
            r#"<Frame width={100} height={100}>
  <Rect x={10} y={-5} width={20} height={20} /><Rect x={50} y={60} width={5} height={5} />
</Frame>"#,
        );
        assert_eq!(f[1], at(10.0, -5.0, 20.0, 20.0));
        assert_eq!(f[2], at(50.0, 60.0, 5.0, 5.0));
    }

    #[test]
    fn a_vector_hugs_its_view_box_and_keeps_its_aspect() {
        let f = frames(
            r#"<Frame layout="row">
  <Vector d="M0 0L24 12" viewBox="0 0 24 12" /><Vector d="M0 0L24 12" viewBox="0 0 24 12" height={6} />
</Frame>"#,
        );
        assert_eq!(f[1], at(0.0, 0.0, 24.0, 12.0));
        assert_eq!(f[2], at(24.0, 0.0, 12.0, 6.0));
    }

    #[test]
    fn centered_text_sits_in_the_middle_of_its_width() {
        let left = frames(r#"<Frame><Text width={300}>Hi</Text></Frame>"#);
        let s = scene(
            r##"<Frame width={300} height={40} fill="#ffffff"><Text width={300} textAlign="center" color="#000000">Hi</Text></Frame>"##,
        );
        let p = Renderer::new().paint(&s, 1.0);
        let dark = |x: u16| (0..20).any(|y| p.sample(x, y).r < 128);
        assert_eq!(left[1].width, 300.0);
        assert!((140..160).any(dark), "ink near the middle");
        assert!(
            !(0..120).any(dark) && !(180..300).any(dark),
            "none near the edges"
        );
    }

    fn rgba(p: &Pixmap, x: u16, y: u16) -> [u8; 4] {
        let c = p.sample(x, y).to_u8_array();
        // Unpremultiply, to compare with the colors a design names.
        if c[3] == 0 {
            return [0; 4];
        }
        let u = |v: u8| ((v as u32 * 255 + c[3] as u32 / 2) / c[3] as u32) as u8;
        [u(c[0]), u(c[1]), u(c[2]), c[3]]
    }

    fn close(a: [u8; 4], b: [u8; 4]) -> bool {
        a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 3)
    }

    #[test]
    fn a_gradient_runs_along_its_angle() {
        let s = scene(
            r##"<Frame width={100} height={20} fill="linear-gradient(90deg, #ff0000 0%, #0000ff 100%)" />"##,
        );
        let p = Renderer::new().paint(&s, 1.0);
        assert!(
            close(rgba(&p, 0, 10), [255, 0, 0, 255]),
            "{:?}",
            rgba(&p, 0, 10)
        );
        assert!(
            close(rgba(&p, 99, 10), [0, 0, 255, 255]),
            "{:?}",
            rgba(&p, 99, 10)
        );
        let down = scene(
            r##"<Frame width={20} height={100} fill="linear-gradient(180deg, #ff0000 0%, #0000ff 100%)" />"##,
        );
        let p = Renderer::new().paint(&down, 1.0);
        assert!(close(rgba(&p, 10, 0), [255, 0, 0, 255]));
        assert!(close(rgba(&p, 10, 99), [0, 0, 255, 255]));
    }

    #[test]
    fn a_stroke_is_drawn_inside_the_box() {
        let s = scene(
            r##"<Frame width={40} height={40}><Rect x={10} y={10} width={20} height={20} stroke="#ff0000" strokeWidth={4} /></Frame>"##,
        );
        let p = Renderer::new().paint(&s, 1.0);
        assert!(close(rgba(&p, 11, 20), [255, 0, 0, 255]));
        assert!(close(rgba(&p, 13, 20), [255, 0, 0, 255]));
        assert_eq!(rgba(&p, 9, 20)[3], 0, "nothing outside the box");
        assert_eq!(rgba(&p, 20, 20)[3], 0, "nothing in the middle");
    }

    #[test]
    fn each_corner_takes_its_own_radius() {
        let s =
            scene(r##"<Frame width={40} height={40} radius={[20, 0, 0, 0]} fill="#000000" />"##);
        let p = Renderer::new().paint(&s, 1.0);
        assert_eq!(rgba(&p, 1, 1)[3], 0, "top left is rounded");
        assert_eq!(rgba(&p, 39, 0)[3], 255, "top right is square");
        assert_eq!(rgba(&p, 0, 39)[3], 255, "bottom left is square");
    }

    #[test]
    fn opacity_applies_to_the_subtree_as_one() {
        // Two overlapping opaque squares at half opacity: where they overlap
        // is as light as where they don't, because the group fades as one.
        let s = scene(
            r##"<Frame width={30} height={10} fill="#ffffff"><Frame opacity={0.5}>
  <Rect width={20} height={10} fill="#000000" /><Rect x={10} width={20} height={10} fill="#000000" />
</Frame></Frame>"##,
        );
        let p = Renderer::new().paint(&s, 1.0);
        let (alone, overlap) = (rgba(&p, 5, 5), rgba(&p, 15, 5));
        assert!(close(alone, overlap), "{alone:?} {overlap:?}");
        assert!(alone[0] > 100 && alone[0] < 155, "{alone:?}");
    }

    #[test]
    fn clip_cuts_children_to_the_box() {
        let s = scene(
            r##"<Frame width={40} height={40}><Frame width={20} height={20} clip>
  <Rect x={10} y={10} width={30} height={30} fill="#000000" />
</Frame></Frame>"##,
        );
        let p = Renderer::new().paint(&s, 1.0);
        assert_eq!(rgba(&p, 15, 15)[3], 255);
        assert_eq!(rgba(&p, 25, 25)[3], 0);
    }

    #[test]
    fn a_vector_fills_its_path_scaled_into_its_box() {
        // A triangle in a 10×10 view box, drawn at 40×40.
        let s = scene(
            r##"<Frame width={40} height={40}><Vector d="M0 0L10 0L0 10Z" viewBox="0 0 10 10" width={40} height={40} fill="#000000" /></Frame>"##,
        );
        let p = Renderer::new().paint(&s, 1.0);
        assert_eq!(rgba(&p, 5, 5)[3], 255);
        assert_eq!(rgba(&p, 35, 35)[3], 0);
    }

    fn png_bytes(w: u32, h: u32, px: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut w_ = e.write_header().unwrap();
        let mut data = Vec::new();
        for y in 0..h {
            for x in 0..w {
                data.extend(px(x, y));
            }
        }
        w_.write_image_data(&data).unwrap();
        drop(w_);
        out
    }

    #[test]
    fn an_image_covers_its_box() {
        // A 2×1 image, left half red and right half blue, covering a 20×20
        // box: scaled to 40×20 and centered, so the box shows the middle.
        let image = png_bytes(2, 1, |x, _| {
            if x == 0 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            }
        });
        let mut s = scene(r#"<Frame width={20} height={20} />"#);
        s.root.kind = Kind::Image;
        s.root.image = Some(resolve::Image {
            src: "i.png".into(),
            data: image,
        });
        // The image is sampled smoothly, so each side is mostly its color.
        let p = Renderer::new().paint(&s, 1.0);
        let (left, right) = (rgba(&p, 1, 10), rgba(&p, 18, 10));
        assert!(left[0] > 200 && left[2] < 50, "{left:?}");
        assert!(right[2] > 200 && right[0] < 50, "{right:?}");
        assert_eq!((left[3], right[3]), (255, 255));
        assert_eq!(Renderer::new().layout(&s)[0], at(0.0, 0.0, 20.0, 20.0));
    }

    #[test]
    fn png_and_jpeg_decode() {
        let png = decode(&png_bytes(3, 2, |_, _| [1, 2, 3, 4])).unwrap();
        assert_eq!((png.width, png.height, png.data.data().len()), (3, 2, 24));
        let jpeg = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../examples/checkout/shirt.jpg"
        ))
        .unwrap();
        let jpeg = decode(&jpeg).unwrap();
        assert_eq!((jpeg.width, jpeg.height), (160, 160));
        assert_eq!(jpeg.data.data().len(), 160 * 160 * 4);
        assert!(decode(b"\x89PNG\r\n\x1a\nbroken").is_none());
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

#[cfg(test)]
mod examples {
    use std::path::PathBuf;

    use super::*;
    use crate::Files;

    /// The repository's files, read from disk as the server reads them.
    struct Repo(PathBuf);

    impl Files for Repo {
        fn bytes(&self, path: &str) -> Option<Vec<u8>> {
            std::fs::read(self.0.join(path)).ok()
        }
    }

    fn repo() -> Repo {
        Repo(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".."))
    }

    /// Each example design, painted natively, matches its reference PNG in
    /// `design/testdata/` within [`TOLERANCE`]. After a deliberate change to
    /// how designs look, `DEVPAGE_UPDATE_REFERENCES=1 cargo test` rewrites
    /// them — and the diff shows the new drawings for review.
    #[test]
    fn the_examples_paint_as_their_references() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata");
        let update = std::env::var_os("DEVPAGE_UPDATE_REFERENCES").is_some();
        let mut renderer = Renderer::new();
        for name in ["button", "checkout", "confirmation", "details"] {
            let path = format!("examples/checkout/{name}.design");
            let scene = crate::resolve(&repo(), &path).unwrap_or_else(|p| panic!("{p:?}"));
            let png = crate::png(&mut renderer, &scene);
            let reference = dir.join(format!("{name}.png"));
            if update {
                std::fs::write(&reference, &png).unwrap();
                continue;
            }
            let expected = decode(&std::fs::read(&reference).unwrap()).unwrap();
            let painted = decode(&png).unwrap();
            assert_eq!(
                (painted.width, painted.height),
                (expected.width, expected.height),
                "{name}"
            );
            if let Err(share) = matches(painted.data.data(), expected.data.data()) {
                panic!("{name}: {:.2}% of pixels differ", share * 100.0);
            }
        }
    }

    #[test]
    fn matching_tolerates_edges_only() {
        let a = vec![100u8; 4 * 1000];
        let mut b = a.clone();
        b[0] = 100 + EDGE; // within the edge allowance
        assert!(matches(&a, &b).is_ok());
        for px in b.chunks_mut(4).take(10) {
            px[1] = 0; // 1% of pixels far off: still within tolerance
        }
        assert!(matches(&a, &b).is_ok());
        b[4 * 10 + 2] = 0; // one more pixel tips it over
        assert_eq!(matches(&a, &b), Err(0.011));
        assert_eq!(matches(&a, &a[4..]), Err(1.0));
    }
}
