//! Scene → pixels. The engine lays a scene out once (taffy, parley) and
//! turns it into a display list of vello drawing commands; a backend draws
//! the list — `vello_cpu` here, for the PNG.

use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontWeight, Layout as Text,
    LayoutContext, LineHeight, PositionedLayoutItem, StyleProperty,
};
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

/// Inter's ascender and descender, as fractions of the font size: how far a
/// line's glyphs can reach above and below its baseline.
const INTER_ASCENT: f64 = 0.969;
const INTER_DESCENT: f64 = 0.242;

/// The largest drawing a design may have, in pixels a side at 1×: far beyond
/// any screen, and a PNG of it still fits in memory.
pub const MAX_SIDE: f32 = 16384.0;

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
    Glyphs(FontData, f32, PColor, Vec<Glyph>),
    PushClip(BezPath),
    PushOpacity(f32),
    Pop,
}

/// A scene laid out and ready to paint: every node's box in tree order, the
/// design's drawing, and the commands that paint it.
pub struct Drawing {
    pub frames: Vec<Frame>,
    pub area: Frame,
    pub list: Vec<Draw>,
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
        let mut hugs = Vec::new();
        let root = build(&mut tree, &scene.root, None, &mut texts, &mut hugs);
        // A frame with no layout places its children absolutely, so flexbox
        // gives a hugging one no size; it takes its children's extent
        // instead. `hugs` is in post-order, so a pass settles every frame
        // whose children are settled, and each pass settles at least one
        // more level — so `hugs.len()` passes after the first always suffice.
        for _ in 0..=hugs.len() {
            self.compute(&mut tree, root, &texts);
            let mut changed = false;
            for &(id, w, h) in &hugs {
                let (mut ew, mut eh) = (0.0f32, 0.0f32);
                for child in tree.children(id).expect("children") {
                    let l = tree.layout(child).expect("laid out");
                    ew = ew.max(l.location.x + l.size.width);
                    eh = eh.max(l.location.y + l.size.height);
                }
                let mut style = tree.style(id).expect("style").clone();
                let mut set = |d: &mut Dimension, v: f32| {
                    if *d != Dimension::length(v) {
                        *d = Dimension::length(v);
                        changed = true;
                    }
                };
                if w {
                    set(&mut style.size.width, ew);
                }
                if h {
                    set(&mut style.size.height, eh);
                }
                tree.set_style(id, style).expect("style");
            }
            if !changed {
                break;
            }
        }
        let mut frames = Vec::new();
        collect(&tree, root, 0.0, 0.0, &mut frames);
        frames
    }

    fn compute(
        &mut self,
        tree: &mut TaffyTree<Option<Leaf>>,
        root: NodeId,
        texts: &[(resolve::TextStyle, bool)],
    ) {
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
    }

    /// The scene as drawing commands — laid out once, shaped once — and the
    /// area they cover: the design's **drawing**, its root's box plus
    /// anything painted outside it, in whole pixels. The list is clipped to
    /// that area, so every backend shows exactly it.
    pub fn display_list(&mut self, scene: &Scene) -> (Frame, Vec<Draw>) {
        let frames = self.layout(scene);
        self.list_for(scene, &frames)
    }

    /// The scene laid out once and turned into its display list, or why it
    /// can't be drawn: a drawing bigger than `MAX_SIDE` a side. What the PNG,
    /// the canvas and `devpage check` all start from.
    pub fn drawing(&mut self, scene: &Scene) -> Result<Drawing, String> {
        let frames = self.layout(scene);
        let (area, list) = self.list_for(scene, &frames);
        if area.width > MAX_SIDE || area.height > MAX_SIDE {
            return Err(format!(
                "the drawing is {}×{} pixels; a design draws at most {} a side",
                area.width, area.height, MAX_SIDE
            ));
        }
        Ok(Drawing { frames, area, list })
    }

    /// The display list for a scene already laid out as `frames`.
    fn list_for(&mut self, scene: &Scene, frames: &[Frame]) -> (Frame, Vec<Draw>) {
        let root = frames[0];
        let mut area = KRect::new(
            root.x as f64,
            root.y as f64,
            (root.x + root.width) as f64,
            (root.y + root.height) as f64,
        );
        let mut list = Vec::new();
        let mut at = 0;
        self.node(&scene.root, frames, &mut at, &mut list, None, &mut area);
        // Out to whole pixels, ignoring the slivers floating point leaves on
        // a curve's bounds.
        const SLIVER: f64 = 0.01;
        let area = KRect::new(
            (area.x0 + SLIVER).floor(),
            (area.y0 + SLIVER).floor(),
            (area.x1 - SLIVER).ceil(),
            (area.y1 - SLIVER).ceil(),
        );
        list.insert(0, Draw::PushClip(area.to_path(0.1)));
        list.push(Draw::Pop);
        let area = Frame {
            x: area.x0 as f32,
            y: area.y0 as f32,
            width: area.width() as f32,
            height: area.height() as f32,
        };
        (area, list)
    }

    /// Paints the scene's drawing with `vello_cpu` at `scale` device pixels
    /// per scene pixel, its top-left at the drawing's. A drawing bigger than
    /// `MAX_SIDE` a side is painted only that far.
    pub fn paint(&mut self, scene: &Scene, scale: f32) -> Pixmap {
        let (area, list) = self.display_list(scene);
        paint_list(area, &list, scale)
    }

    /// Adds `node`'s commands to `list`, and what they cover — inside `clip`
    /// — to `area`.
    fn node(
        &mut self,
        node: &Node,
        frames: &[Frame],
        at: &mut usize,
        list: &mut Vec<Draw>,
        clip: Option<KRect>,
        area: &mut KRect,
    ) {
        let f = frames[*at];
        *at += 1;
        let mut cover = |r: KRect| {
            let r = clip.map_or(r, |c| r.intersect(c));
            if r.width() > 0.0 && r.height() > 0.0 {
                *area = area.union(r);
            }
        };
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
            // The box's own shape, widened by the spread — every corner's
            // radius by it too — and moved by the offset.
            let spread = s.spread as f64;
            let r = rect.inflate(spread, spread);
            let mut path = if node.kind == Kind::Ellipse {
                KEllipse::from_rect(r).to_path(0.1)
            } else {
                let [tl, tr, br, bl] = node.radius.map(|v| (v as f64 + spread).max(0.0));
                RoundedRect::from_rect(r, (tl, tr, br, bl)).to_path(0.1)
            };
            path.apply_affine(Affine::translate((s.x as f64, s.y as f64)));
            let std_dev = (s.blur as f64 / 2.0).max(0.0);
            // A blur reaches three standard deviations past its shape.
            cover(path.bounding_box().inflate(3.0 * std_dev, 3.0 * std_dev));
            list.push(shadow(&path, std_dev, s.color));
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
            cover(path.bounding_box());
            list.push(Draw::Fill(path, paint));
        }
        if let Some(image) = &node.image {
            let data = image.pixels.clone();
            // Covering the box: scaled to fill it, centered, cut to its shape.
            let (iw, ih) = (data.width as f64, data.height as f64);
            let s = (f.width as f64 / iw).max(f.height as f64 / ih);
            let place = Affine::translate((
                f.x as f64 + (f.width as f64 - iw * s) / 2.0,
                f.y as f64 + (f.height as f64 - ih * s) / 2.0,
            )) * Affine::scale(s);
            cover(rect);
            list.push(Draw::Fill(shape(0.0), Paint::Image(data, place)));
        }
        if let Some(c) = node.stroke {
            let path = match &vector {
                Some(path) => path.clone(),
                None => shape(node.stroke_width as f64 / 2.0),
            };
            let half = node.stroke_width as f64 / 2.0;
            cover(path.bounding_box().inflate(half, half));
            list.push(Draw::Stroke(path, node.stroke_width, color(c)));
        }
        if let Some(text) = &node.text {
            let layout = self.text_layout(text, Some(f.width + 0.5));
            for line in layout.lines() {
                let m = line.metrics();
                let size = text.size as f64;
                let baseline = f.y as f64 + m.baseline as f64;
                cover(KRect::new(
                    f.x as f64 + m.inline_min_coord as f64,
                    (f.y as f64 + m.block_min_coord as f64).min(baseline - INTER_ASCENT * size),
                    f.x as f64 + m.inline_max_coord as f64,
                    (f.y as f64 + m.block_max_coord as f64).max(baseline + INTER_DESCENT * size),
                ));
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
        let inner = if node.clip {
            list.push(Draw::PushClip(shape(0.0)));
            Some(clip.map_or(rect, |c| rect.intersect(c)))
        } else {
            clip
        };
        for child in &node.children {
            self.node(child, frames, at, list, inner, area);
        }
        if node.clip {
            list.push(Draw::Pop);
        }
        if opacity {
            list.push(Draw::Pop);
        }
    }
}

/// The most samples a blurred shadow is drawn from.
const SHADOW_SAMPLES: f64 = (1 << 20) as f64;

/// A shadow: `path` filled with `c`, blurred by a Gaussian of `std_dev`
/// pixels. Neither vello's blurred rounded rectangle, which takes one radius
/// for all four corners, nor any filter both backends share draws that, so
/// the engine blurs the shape itself: it samples the shape's coverage every
/// half standard deviation — coarser only past [`SHADOW_SAMPLES`] samples —
/// blurs that with a true Gaussian, and paints the result as an image over
/// the shadow's extent. A blur that wide has nothing finer than its samples,
/// so the image, sampled smoothly, is as sharp as the shadow at any zoom.
fn shadow(path: &BezPath, std_dev: f64, c: Color) -> Draw {
    if std_dev == 0.0 {
        return Draw::Fill(path.clone(), Paint::Solid(color(c)));
    }
    let bounds = path.bounding_box().inflate(3.0 * std_dev, 3.0 * std_dev);
    let step = (std_dev / 2.0)
        .max((bounds.area() / SHADOW_SAMPLES).sqrt())
        .max(bounds.width().max(bounds.height()) / 4096.0);
    let (w, h) = (
        (bounds.width() / step).ceil().max(1.0) as usize,
        (bounds.height() / step).ceil().max(1.0) as usize,
    );
    // The shape's coverage of each sample.
    let mut ctx = RenderContext::new(w as u16, h as u16);
    ctx.set_transform(Affine::scale(1.0 / step) * Affine::translate((-bounds.x0, -bounds.y0)));
    ctx.set_paint(PColor::from_rgba8(0, 0, 0, 255));
    ctx.fill_path(path);
    ctx.flush();
    let mut coverage = Pixmap::new(w as u16, h as u16);
    ctx.render(&mut coverage, &mut Resources::default());
    let mut alpha: Vec<f32> = coverage
        .data_as_u8_slice()
        .iter()
        .skip(3)
        .step_by(4)
        .map(|&a| a as f32 / 255.0)
        .collect();
    // A Gaussian is separable: across each row, then down each column.
    let sigma = std_dev / step;
    let reach = (3.0 * sigma).ceil() as isize;
    let kernel: Vec<f32> = (-reach..=reach)
        .map(|i| (-(i as f64).powi(2) / (2.0 * sigma * sigma)).exp() as f32)
        .collect();
    let total: f32 = kernel.iter().sum();
    let kernel: Vec<f32> = kernel.iter().map(|k| k / total).collect();
    let blur = |alpha: &[f32], at: &dyn Fn(usize, usize) -> usize, lines: usize, len: usize| {
        let mut out = vec![0.0f32; alpha.len()];
        for line in 0..lines {
            for i in 0..len {
                let mut sum = 0.0;
                for (k, weight) in kernel.iter().enumerate() {
                    let j = i as isize + k as isize - reach;
                    if (0..len as isize).contains(&j) {
                        sum += weight * alpha[at(line, j as usize)];
                    }
                }
                out[at(line, i)] = sum;
            }
        }
        out
    };
    alpha = blur(&alpha, &|y, x| y * w + x, h, w);
    alpha = blur(&alpha, &|x, y| y * w + x, w, h);
    let pixels: Vec<u8> = alpha
        .iter()
        .flat_map(|a| {
            [
                c.r,
                c.g,
                c.b,
                (a.clamp(0.0, 1.0) * c.a as f32).round() as u8,
            ]
        })
        .collect();
    let image = ImageData {
        data: Blob::new(Arc::new(pixels)),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width: w as u32,
        height: h as u32,
    };
    let extent = KRect::new(
        bounds.x0,
        bounds.y0,
        bounds.x0 + w as f64 * step,
        bounds.y0 + h as f64 * step,
    );
    let place = Affine::translate((bounds.x0, bounds.y0)) * Affine::scale(step);
    Draw::Fill(extent.to_path(0.1), Paint::Image(image, place))
}

/// Replays a display list with `vello_cpu` into a pixmap of its area at
/// `scale`, as `Renderer::paint` does.
pub fn paint_list(area: Frame, list: &[Draw], scale: f32) -> Pixmap {
    let side = |v: f32| ((v.min(MAX_SIDE) * scale).ceil().clamp(1.0, u16::MAX as f32)) as u16;
    let (w, h) = (side(area.width), side(area.height));
    let mut ctx = RenderContext::new(w, h);
    let mut resources = Resources::default();
    ctx.set_transform(
        Affine::scale(scale as f64) * Affine::translate((-area.x as f64, -area.y as f64)),
    );
    for draw in list {
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
/// How far apart two drawings of the same scene may be. Two rasterizers of one
/// display list differ only in how they anti-alias an edge: a pixel on an edge
/// takes some value between the colors either side of it, and each rasterizer
/// picks a slightly different one. So a pixel is **unexplained** when, in some
/// channel, its value lies more than [`EDGE`] outside the range of the other
/// drawing's pixels around it (itself and its eight neighbours), in either
/// direction. A missing or moved part — a divider, an icon, a word — leaves
/// pixels no neighbour in the other drawing explains. At most [`SHARE`] of the
/// pixels may be unexplained, and no 8-connected cluster of them may be larger
/// than [`CLUSTER`].
pub const EDGE: u8 = 8;
/// The largest share of pixels that may be unexplained.
pub const SHARE: f64 = 0.0005;
/// The largest cluster of touching unexplained pixels.
pub const CLUSTER: usize = 4;

/// How two drawings of the same size differ.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Difference {
    /// The share of pixels that are unexplained.
    pub share: f64,
    /// The size of the largest 8-connected cluster of unexplained pixels.
    pub cluster: usize,
    /// The largest difference in any channel between the same pixel of each.
    pub max: u8,
}

impl Difference {
    /// Whether this is only the anti-aliasing two rasterizers may differ by.
    pub fn within_tolerance(&self) -> bool {
        self.share <= SHARE && self.cluster <= CLUSTER
    }
}

/// How two RGBA buffers `width` pixels wide differ; `None` when their sizes
/// differ.
pub fn compare(a: &[u8], b: &[u8], width: usize) -> Option<Difference> {
    if a.len() != b.len() || width == 0 || !a.len().is_multiple_of(4 * width) {
        return None;
    }
    let count = a.len() / 4;
    let height = count / width;
    let neighbours = |i: usize| {
        let (x, y) = (i % width, i / width);
        let xs = x.saturating_sub(1)..=(x + 1).min(width - 1);
        (y.saturating_sub(1)..=(y + 1).min(height - 1))
            .flat_map(move |ny| xs.clone().map(move |nx| ny * width + nx))
    };
    // Whether pixel `i` of `p` lies outside what `q` has around it.
    let outside = |p: &[u8], q: &[u8], i: usize| {
        (0..4).any(|c| {
            let v = p[4 * i + c];
            let (lo, hi) = neighbours(i).fold((u8::MAX, 0), |(lo, hi), n| {
                (lo.min(q[4 * n + c]), hi.max(q[4 * n + c]))
            });
            v.saturating_add(EDGE) < lo || v > hi.saturating_add(EDGE)
        })
    };
    let max = a
        .iter()
        .zip(b)
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0);
    let unexplained: Vec<bool> = (0..count)
        .map(|i| outside(a, b, i) || outside(b, a, i))
        .collect();
    let mut seen = vec![false; count];
    let (mut total, mut cluster) = (0, 0);
    let mut stack = Vec::new();
    for start in 0..count {
        if !unexplained[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        let mut size = 0;
        while let Some(i) = stack.pop() {
            size += 1;
            for n in neighbours(i) {
                if unexplained[n] && !seen[n] {
                    seen[n] = true;
                    stack.push(n);
                }
            }
        }
        total += size;
        cluster = cluster.max(size);
    }
    Some(Difference {
        share: total as f64 / count.max(1) as f64,
        cluster,
        max,
    })
}

/// Whether two RGBA buffers `width` pixels wide are the same drawing within
/// the tolerance; otherwise how they differ, or `None` for different sizes.
pub fn matches(a: &[u8], b: &[u8], width: usize) -> Result<(), Option<Difference>> {
    match compare(a, b, width) {
        Some(d) if d.within_tolerance() => Ok(()),
        other => Err(other),
    }
}

/// A PNG's or JPEG's pixels as straight RGBA, with its width and height —
/// for comparing drawings.
pub fn pixels(data: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let image = resolve::decode(data).ok()?;
    Some((image.data.data().to_vec(), image.width, image.height))
}

fn build(
    tree: &mut TaffyTree<Option<Leaf>>,
    node: &Node,
    parent: Option<&Node>,
    texts: &mut Vec<(resolve::TextStyle, bool)>,
    hugs: &mut Vec<(NodeId, bool, bool)>,
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
        (_, Some(i)) => Some((i.pixels.width as f32, i.pixels.height as f32)),
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
        .map(|c| build(tree, c, Some(node), texts, hugs))
        .collect();
    let id = tree.new_with_children(style, &children).expect("node");
    // A root has no parent space to fill: on its own, `"fill"` hugs.
    let hugs_by = |s: DSize| s == DSize::Hug || (parent.is_none() && s == DSize::Fill);
    let (w, h) = (hugs_by(node.width), hugs_by(node.height));
    if node.layout == Layout::None && !node.children.is_empty() && (w || h) {
        hugs.push((id, w, h));
    }
    id
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

    /// A shadow takes the shape of its box: every corner its own radius, each
    /// widened by the spread.
    #[test]
    fn a_shadow_follows_each_corner_and_the_spread() {
        // The shadow's box is 55..95 × 5..45: its top-left corner rounded by
        // 10 + 5, the others by 0 + 5.
        let sharp = scene(
            r##"<Frame width={100} height={60}><Rect x={10} y={10} width={30} height={30} radius={[10, 0, 0, 0]} shadow={{ x: 50, spread: 5, color: "#000000" }} /></Frame>"##,
        );
        let p = Renderer::new().paint(&sharp, 1.0);
        assert_eq!(rgba(&p, 57, 7)[3], 0, "top left is rounded by 15");
        assert_eq!(rgba(&p, 92, 7)[3], 255, "top right only by 5");
        assert_eq!(rgba(&p, 92, 42)[3], 255, "bottom right only by 5");
        assert_eq!(rgba(&p, 57, 42)[3], 255, "bottom left only by 5");
        assert_eq!(rgba(&p, 94, 5)[3], 0, "but rounded");
        // Blurred, each corner still fades by its own radius.
        let blurred = scene(
            r##"<Frame width={100} height={60}><Rect x={10} y={10} width={30} height={30} radius={[20, 0, 0, 0]} shadow={{ x: 50, blur: 6, color: "#000000" }} /></Frame>"##,
        );
        let p = Renderer::new().paint(&blurred, 1.0);
        let (tl, tr) = (rgba(&p, 63, 13)[3], rgba(&p, 86, 13)[3]);
        assert!(tr > 150 && tl < 100, "top left {tl}, top right {tr}");
        let (bl, br) = (rgba(&p, 63, 36)[3], rgba(&p, 86, 36)[3]);
        assert!(
            bl.abs_diff(tr) <= 2 && br.abs_diff(tr) <= 2,
            "{bl} {br} {tr}"
        );
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
        s.root.image = Some(resolve::Image::new("i.png".into(), image).unwrap());
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
        let png = resolve::decode(&png_bytes(3, 2, |_, _| [1, 2, 3, 4])).unwrap();
        assert_eq!((png.width, png.height, png.data.data().len()), (3, 2, 24));
        let jpeg = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../docs/designs/checkout/shirt.jpg"
        ))
        .unwrap();
        let jpeg = resolve::decode(&jpeg).unwrap();
        assert_eq!((jpeg.width, jpeg.height), (160, 160));
        assert_eq!(jpeg.data.data().len(), 160 * 160 * 4);
        assert!(resolve::decode(b"\x89PNG\r\n\x1a\nbroken").is_err());
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

    #[test]
    fn a_hugging_frame_with_no_layout_takes_its_childrens_extent() {
        let s = scene(
            r##"<Frame fill="#ff0000"><Rect x={10} y={5} width={100} height={50} fill="#0000ff" /></Frame>"##,
        );
        let mut r = Renderer::new();
        let frames = r.layout(&s);
        assert_eq!((frames[0].width, frames[0].height), (110.0, 55.0));
        // Nested: the inner frame settles first, then the outer one.
        let s = scene(
            r##"<Frame><Frame x={20} y={0}><Rect x={0} y={30} width={10} height={10} /></Frame></Frame>"##,
        );
        let frames = r.layout(&s);
        assert_eq!((frames[0].width, frames[0].height), (30.0, 40.0));
        assert_eq!((frames[1].width, frames[1].height), (10.0, 40.0));
        // A fixed side stays fixed; only the hugging one follows the children.
        let s = scene(r##"<Frame width={300}><Rect width={10} height={70} /></Frame>"##);
        let frames = r.layout(&s);
        assert_eq!((frames[0].width, frames[0].height), (300.0, 70.0));
    }

    #[test]
    fn the_drawing_is_the_root_plus_what_is_painted_outside_it() {
        let mut r = Renderer::new();
        // Paint inside the root: the drawing is exactly the root.
        let s = scene(
            r##"<Frame width={40} height={30} radius={12} fill="#000000"><Text>Hi</Text></Frame>"##,
        );
        let (area, _) = r.display_list(&s);
        assert_eq!(
            area,
            Frame {
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 30.0
            }
        );
        assert_eq!(r.paint(&s, 1.0).width(), 40);
        // A shadow and a child past the edge widen it; the PNG shows them.
        let s = scene(
            r##"<Frame width={40} height={30} fill="#ffffff" shadow={{ y: 4, blur: 8, color: "#000000ff" }}>
  <Rect x={30} y={-10} width={20} height={10} fill="#ff0000" />
</Frame>"##,
        );
        let (area, _) = r.display_list(&s);
        assert_eq!(
            area,
            Frame {
                x: -12.0,
                y: -10.0,
                width: 64.0,
                height: 56.0
            }
        );
        let p = r.paint(&s, 1.0);
        assert_eq!((p.width(), p.height()), (64, 56));
        // The red child, at (30..50, -10..0), drawn at its place in the PNG.
        assert!(close(rgba(&p, 12 + 45, 5), [255, 0, 0, 255]));
        // The shadow, below the box.
        assert!(rgba(&p, 12 + 20, 10 + 30 + 3)[3] > 100);
        // A clipping parent cuts what its children paint, and the area too.
        let s = scene(
            r##"<Frame width={40} height={30} clip><Rect x={30} width={50} height={10} fill="#ff0000" /></Frame>"##,
        );
        assert_eq!(r.display_list(&s).0.width, 40.0);
    }

    #[test]
    fn a_drawing_too_big_is_a_problem_not_a_picture() {
        let mut r = Renderer::new();
        let s = scene(r##"<Frame width={20000} height={10} fill="#000000" />"##);
        assert_eq!(
            crate::png(&mut r, &s).unwrap_err(),
            "the drawing is 20000×10 pixels; a design draws at most 16384 a side"
        );
        // Painting for the canvas stops at the limit rather than allocating more.
        assert_eq!(r.paint(&s, 1.0).width(), 16384);
    }

    #[test]
    fn a_root_that_fills_on_its_own_hugs() {
        let mut r = Renderer::new();
        for root in [
            r##"<Frame width="fill" height="fill" fill="#ff0000"><Rect width={100} height={50} /></Frame>"##,
            r##"<Frame width="fill" layout="row" fill="#ff0000"><Rect width={100} height={50} /></Frame>"##,
        ] {
            let d = r.drawing(&scene(root)).unwrap();
            assert_eq!((d.area.width, d.area.height), (100.0, 50.0), "{root}");
        }
    }

    #[test]
    fn a_drawing_is_laid_out_once_and_refuses_to_be_too_big() {
        let mut r = Renderer::new();
        let s = scene(r#"<Frame layout="row" padding={4}><Rect width={10} height={10} /></Frame>"#);
        let d = r.drawing(&s).unwrap();
        assert_eq!(d.frames, r.layout(&s));
        let (area, list) = r.display_list(&s);
        assert_eq!((d.area, d.list.len()), (area, list.len()));
        let big = scene(r##"<Frame width={20000} height={10} fill="#000000" />"##);
        assert!(
            r.drawing(&big)
                .err()
                .unwrap()
                .contains("at most 16384 a side")
        );
    }

    #[test]
    fn no_input_makes_the_engine_panic() {
        let attrs = [
            "fill",
            "stroke",
            "strokeWidth",
            "radius",
            "shadow",
            "clip",
            "layout",
            "gap",
            "padding",
            "align",
            "justify",
            "width",
            "height",
            "x",
            "y",
            "opacity",
            "onClick",
            "size",
            "weight",
            "color",
            "lineHeight",
            "textAlign",
            "d",
            "viewBox",
            "src",
            "name",
        ];
        let values = [
            "\"#fff\u{200b}\"",
            "\"#ffé\"",
            "\"€\"",
            "\"\"",
            "\"#\"",
            "\"#12345\"",
            "\"linear-gradient(\"",
            "\"linear-gradient(é)\"",
            "\"linear-gradient(90deg, #fff é%)\"",
            "{-1000000}",
            "{1000000}",
            "{0}",
            "{-0.0001}",
            "{[]}",
            "{[1]}",
            "{[1,2,3]}",
            "{[1000000, 1000000, 1000000, 1000000]}",
            "{{}}",
            "{{ blur: 1000000, spread: 1000000 }}",
            "{{ x: \"é\" }}",
            "{true}",
            "\"navigate \"",
            "\"navigate ../../../x.design\"",
            "\"M\"",
            "\"M0 0 L1000000 1000000 Z\"",
            "\"0 0 0 0\"",
            "\"é é é é\"",
            "\"hug\"",
            "\"fill\"",
            "\"none\"",
            "\"🙂\"",
        ];
        let mut r = Renderer::new();
        for el in ["Frame", "Text", "Rect", "Ellipse", "Vector", "Image"] {
            for attr in attrs {
                for value in values {
                    let source = format!(
                        r#"<Frame layout="row"><{el} {attr}={value}>{}</{el}></Frame>"#,
                        if el == "Text" { "é 🙂" } else { "" }
                    );
                    let files: HashMap<String, String> =
                        [("a.design".to_string(), source.clone())].into();
                    if let Ok(s) = resolve::resolve(&files, "a.design") {
                        r.layout(&s);
                        let _ = crate::png(&mut r, &s);
                    }
                }
            }
        }
        // Truncations of a real design at every character, multi-byte ones
        // included.
        let source = r##"import B from "./b.design"
<Frame props={{ t: "héllo 🙂" }} layout="column" fill="#fff">
  <Text color="#123">{t} — ok</Text><B />
</Frame>"##;
        for (i, _) in source.char_indices() {
            let files: HashMap<String, String> = [
                ("a.design".to_string(), source[..i].to_string()),
                ("b.design".to_string(), "<Frame />".to_string()),
            ]
            .into();
            let _ = resolve::resolve(&files, "a.design");
        }
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
    /// `design/testdata/` within the tolerance [`matches`] applies. After a deliberate change to
    /// how designs look, `DEVPAGE_UPDATE_REFERENCES=1 cargo test` rewrites
    /// them — and the diff shows the new drawings for review.
    #[test]
    fn the_examples_paint_as_their_references() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata");
        let update = std::env::var_os("DEVPAGE_UPDATE_REFERENCES").is_some();
        let mut renderer = Renderer::new();
        for name in ["button", "checkout", "confirmation", "details"] {
            let path = format!("docs/designs/checkout/{name}.design");
            let scene = crate::resolve(&repo(), &path).unwrap_or_else(|p| panic!("{p:?}"));
            let png = crate::png(&mut renderer, &scene).unwrap();
            let reference = dir.join(format!("{name}.png"));
            if update {
                std::fs::write(&reference, &png).unwrap();
                continue;
            }
            let expected = resolve::decode(&std::fs::read(&reference).unwrap()).unwrap();
            let painted = resolve::decode(&png).unwrap();
            assert_eq!(
                (painted.width, painted.height),
                (expected.width, expected.height),
                "{name}"
            );
            if let Err(d) = matches(
                painted.data.data(),
                expected.data.data(),
                painted.width as usize,
            ) {
                panic!("{name}: {d:?}");
            }
        }
    }

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
            pixels(&crate::png(&mut renderer, &scene).unwrap()).unwrap()
        };
        let whole = paint(&repo());
        for (part, hidden) in [
            (
                r##"<Rect name="Divider" width="fill" height={1} fill="#e2e8f0" />"##,
                r##"<Rect name="Divider" width="fill" height={1} fill="#f8fafc" />"##,
            ),
            (
                r##"viewBox="0 0 24 24" width={20} height={20} fill="#64748b""##,
                r##"viewBox="0 0 24 24" width={20} height={20} fill="#ffffff""##,
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
}
