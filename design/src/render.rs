//! Scene → pixels. The engine lays a scene out (taffy, parley) and turns it
//! into a [`Drawing`]: its display list in chunks under property trees, kept
//! and updated by what changes. A backend paints any area of it through
//! [`crate::display::paint`] — `vello_cpu` here, for the PNG and tiles.

use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontWeight, Layout as Text,
    LayoutContext, LineHeight, PositionedLayoutItem, StyleProperty,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use taffy::prelude::*;

use vello_cpu::kurbo::{
    Affine, BezPath, Ellipse as KEllipse, Rect as KRect, RoundedRect, Shape, Vec2,
};
use vello_cpu::peniko::{
    BlendMode, Color as PColor, Compose, FontData, Gradient, ImageBrush, ImageData, Mix,
};
use vello_cpu::{Glyph, Pixmap, RenderContext, Resources};

use crate::display::{Blend, Builder, Chunk, Clip, Effect, Id, List, Order, Transform};
use crate::fonts::{self, FontFiles, FontId};
use crate::resolve::{
    self, Align, Color, Fill, ImageFiles, ImageId, Kind, Layout, Node, Origin, Rgba, Scene,
    Size as DSize, TextAlign, Theme,
};
use crate::shadow::{self, Outline};

const FONTS: [&[u8]; 4] = [
    include_bytes!("../fonts/Inter-Regular.ttf"),
    include_bytes!("../fonts/Inter-Medium.ttf"),
    include_bytes!("../fonts/Inter-SemiBold.ttf"),
    include_bytes!("../fonts/Inter-Bold.ttf"),
];

/// The fonts and text layout state a renderer keeps between paints.
pub struct Renderer {
    fonts: FontContext,
    /// The embedded Inter faces, each under one blob for the renderer's life.
    inter: Vec<parley::fontique::Blob<u8>>,
    /// The font files registered besides Inter — those of a scene laid out —
    /// or `None` before anything is.
    registered: Option<BTreeSet<FontId>>,
    layouts: LayoutContext<()>,
    /// While a display list is made: the image each image file is painted
    /// from in it, and every image it paints, in the order it first does,
    /// with the attribute that asked for it.
    blobs: HashMap<ImageId, ImageData>,
    painted: Vec<(Painted, Origin)>,
}

/// An image a drawing paints, and what it shows: an image file, known by
/// its id, or a shadow's pixels.
#[derive(Clone)]
struct Painted {
    data: ImageData,
    file: Option<ImageId>,
}

impl Painted {
    /// Whether the two show the same pixels.
    fn same(&self, other: &Painted) -> bool {
        match (self.file, other.file) {
            (Some(a), Some(b)) => a == b,
            (None, None) => {
                (self.data.width, self.data.height) == (other.data.width, other.data.height)
                    && self.data.data.data() == other.data.data.data()
            }
            _ => false,
        }
    }
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

/// One drawing command, in its chunk's coordinates: what both vello backends
/// replay. Every decision about the drawing is made before a command is
/// made; a backend only translates it into its library's call. The layers it
/// is drawn in are nodes of the property trees, not commands.
pub enum Draw {
    Fill(BezPath, Paint),
    Stroke(BezPath, f32, PColor),
    /// A glyph run: its font, size and color, its point on the font's
    /// variation axes — empty for a static font — and its glyphs.
    Glyphs(FontData, f32, PColor, Vec<i16>, Vec<Glyph>),
}

/// Why a scene can't be drawn: images and shadows that don't fit the GPU's
/// atlas — `at` the attribute that broke it, when one did.
#[derive(Debug, Clone, PartialEq)]
pub struct Undrawable {
    pub message: String,
    pub at: Option<Origin>,
}

impl std::fmt::Display for Undrawable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl Renderer {
    pub fn new() -> Renderer {
        let fonts = FontContext {
            collection: parley::fontique::Collection::new(parley::fontique::CollectionOptions {
                shared: false,
                system_fonts: false,
            }),
            source_cache: parley::fontique::SourceCache::default(),
        };
        let mut renderer = Renderer {
            fonts,
            inter: FONTS
                .iter()
                .map(|font| parley::fontique::Blob::new(std::sync::Arc::new(*font)))
                .collect(),
            registered: None,
            layouts: LayoutContext::new(),
            blobs: HashMap::new(),
            painted: Vec::new(),
        };
        renderer.register(&FontFiles::new());
        renderer
    }

    /// Makes the scene's font files available to layout, each under a
    /// family name of its own. The collection keeps what it has while a
    /// scene uses no other font file; otherwise it starts again from Inter
    /// and this scene's, so it never holds more than one scene's fonts.
    fn register(&mut self, files: &FontFiles) {
        if let Some(registered) = &self.registered
            && files.keys().all(|id| registered.contains(id))
        {
            return;
        }
        let collection = &mut self.fonts.collection;
        collection.clear();
        for blob in &self.inter {
            collection.register_fonts(blob.clone(), None);
        }
        for (id, file) in files {
            let family = id.family();
            collection.register_fonts(
                file.blob.clone(),
                Some(parley::fontique::FontInfoOverride {
                    family_name: Some(&family),
                    ..Default::default()
                }),
            );
        }
        self.registered = Some(files.keys().copied().collect());
    }

    fn text_layout(&mut self, text: &resolve::TextStyle, width: Option<f32>) -> Text<()> {
        let mut builder = self
            .layouts
            .ranged_builder(&mut self.fonts, &text.content, 1.0, true);
        let family = match text.font {
            Some(id) => id.family(),
            None => fonts::INTER.to_string(),
        };
        builder.push_default(StyleProperty::FontFamily(FontFamily::named(&family)));
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
        self.register(&scene.fonts);
        let mut tree: TaffyTree<Option<Leaf>> = TaffyTree::new();
        let mut hugs = Vec::new();
        let root = build(&mut tree, &scene.root, None, &mut hugs, &scene.images);
        self.settle(&mut tree, root, &hugs);
        frames_by_order(&tree, root, &scene.root)
            .into_values()
            .collect()
    }

    fn compute(&mut self, tree: &mut TaffyTree<Option<Leaf>>, root: NodeId) {
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
                Some(Some(Leaf::Text(text, wraps))) => {
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
    /// A node's own commands, its box at `f` in its own coordinates — its
    /// shadow, fill, image, stroke and text — into `b`, and what each covers
    /// into `covers`. The images it paints go to `self.painted`, each image
    /// file under the blob `self.blobs` holds for it.
    fn own(
        &mut self,
        scene: &Scene,
        node: &Node,
        f: Frame,
        theme: Theme,
        b: &mut Builder,
        covers: &mut Vec<KRect>,
    ) {
        let rect = KRect::new(
            f.x as f64,
            f.y as f64,
            (f.x + f.width) as f64,
            (f.y + f.height) as f64,
        );
        let shape = |inset: f64| node_shape(node, rect, inset);
        if let Some(s) = &node.shadow {
            // The box's own shape, widened by the spread — every corner's
            // radius by it too — and moved by the offset.
            let spread = s.spread as f64;
            let r = rect
                .inflate(spread, spread)
                .with_origin((rect.x0 - spread + s.x as f64, rect.y0 - spread + s.y as f64));
            let outline = if node.kind == Kind::Ellipse {
                Outline::ellipse(r)
            } else {
                Outline::rounded(r, node.radius.map(|v| (v as f64 + spread).max(0.0)))
            };
            let std_dev = (s.blur as f64 / 2.0).max(0.0);
            if r.width() > 0.0 && r.height() > 0.0 {
                // A blur reaches three standard deviations past its shape.
                covers.push(r.inflate(3.0 * std_dev, 3.0 * std_dev));
            }
            let mut images = Vec::new();
            shadow::shadow(outline, std_dev, s.color.get(theme), b, &mut images);
            self.painted.extend(
                images
                    .into_iter()
                    .map(|data| (Painted { data, file: None }, s.at.clone())),
            );
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
                Fill::Solid(c) => Paint::Solid(color(c.get(theme))),
                Fill::Linear { angle, stops } => {
                    Paint::Gradient(gradient(rect, *angle, stops, theme))
                }
            };
            let path = vector.clone().unwrap_or_else(|| shape(0.0));
            covers.push(path.bounding_box());
            b.draw(Draw::Fill(path, paint));
        }
        if let Some(image) = &node.image {
            let file = &scene.images[&image.id];
            let data = match self.blobs.get(&image.id) {
                Some(data) => data.clone(),
                None => {
                    let data = file.image_data();
                    self.blobs.insert(image.id, data.clone());
                    data
                }
            };
            self.painted.push((
                Painted {
                    data: data.clone(),
                    file: Some(image.id),
                },
                image.at.clone(),
            ));
            // Covering the box: the file at its own size scaled to fill it,
            // centered, cut to its shape — drawn from its pixels, which may
            // be fewer.
            let (iw, ih) = (file.width as f64, file.height as f64);
            let s = (f.width as f64 / iw).max(f.height as f64 / ih);
            let place = Affine::translate((
                f.x as f64 + (f.width as f64 - iw * s) / 2.0,
                f.y as f64 + (f.height as f64 - ih * s) / 2.0,
            )) * Affine::scale(s)
                * Affine::scale_non_uniform(
                    iw / file.pixels_width as f64,
                    ih / file.pixels_height as f64,
                );
            covers.push(rect);
            b.draw(Draw::Fill(shape(0.0), Paint::Image(data, place)));
        }
        if let Some(c) = node.stroke {
            let path = match &vector {
                Some(path) => path.clone(),
                None => shape(node.stroke_width as f64 / 2.0),
            };
            let half = node.stroke_width as f64 / 2.0;
            covers.push(path.bounding_box().inflate(half, half));
            b.draw(Draw::Stroke(path, node.stroke_width, color(c.get(theme))));
        }
        if let Some(text) = &node.text {
            let layout = self.text_layout(text, Some(f.width + 0.5));
            for line in layout.lines() {
                // The line's box, or as far as its fonts' ascenders and
                // descenders reach past it.
                let m = line.metrics();
                let y = |v: f32| f.y as f64 + v as f64;
                covers.push(KRect::new(
                    f.x as f64 + m.inline_min_coord as f64,
                    y(m.block_min_coord.min(m.content_block_min_coord)),
                    f.x as f64 + m.inline_max_coord as f64,
                    y(m.block_max_coord.max(m.content_block_max_coord)),
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
                        b.draw(Draw::Glyphs(
                            run.run().font().clone(),
                            run.run().font_size(),
                            color(text.color.get(theme)),
                            run.run()
                                .normalized_coords()
                                .iter()
                                .map(|c| c.to_bits())
                                .collect(),
                            glyphs,
                        ));
                    }
                }
            }
        }
    }
}

/// A scene laid out, in one theme, and made into its display list: kept, so
/// a later scene updates it by what changed. Its layout tree, every node's
/// box by its id, the property-tree nodes each node made, the list, and the
/// images the list paints.
pub struct Drawing {
    tree: TaffyTree<Option<Leaf>>,
    root: NodeId,
    theme: Theme,
    frames: BTreeMap<Order, Frame>,
    recs: BTreeMap<Order, Rec>,
    /// The display list: chunks under property trees, and their index.
    pub list: List,
    /// The blob each image file is painted from in this drawing.
    blobs: HashMap<ImageId, ImageData>,
    /// Every image the list paints, each under a blob of its own, in the
    /// order the list first paints it: what the GPU's atlas holds for this
    /// drawing, placed as [`crate::atlas::place`] places them.
    pub images: Vec<ImageData>,
    /// What each of `images` shows.
    shows: Vec<Painted>,
}

/// What an update to a drawing touched, in the design's coordinates: each
/// chunk it changed, where it was and where it is — and the strips the
/// drawing's area gained or lost. `all` when the area's top-left moved, which
/// moves every tile.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Damage {
    pub rects: Vec<KRect>,
    pub all: bool,
}

/// The property-tree nodes a scene node made — its transform, its opacity's
/// effect and its clip — and the images its own commands paint, in order.
#[derive(Clone)]
struct Rec {
    transform: Id,
    effect: Option<Id>,
    clip: Option<Id>,
    painted: Vec<(Painted, Origin)>,
}

impl Drawing {
    /// The design's drawing: its root's box plus anything painted outside
    /// it, cut by any clipping frame, in whole pixels.
    pub fn area(&self) -> Frame {
        self.list.area
    }

    /// Every node's box, in tree order.
    pub fn frames(&self) -> Vec<Frame> {
        self.frames.values().copied().collect()
    }

    pub fn theme(&self) -> Theme {
        self.theme
    }

    /// The interaction of the topmost node at `(x, y)` — in the design's
    /// coordinates — that has one, where its clipping frames show it.
    pub fn action_at(&self, x: f32, y: f32) -> Option<&str> {
        self.list.action_at(x as f64, y as f64)
    }

    /// Updates the drawing from `old`, the scene it shows, to `new`: the
    /// subtrees the patch between them replaces are laid out and painted
    /// again, nodes that only moved keep their chunks. The damage it
    /// returns is what changed; or why `new` can't be painted — its images
    /// too many for the GPU's atlas — in which case the drawing is `new`'s
    /// all the same.
    pub fn update(
        &mut self,
        renderer: &mut Renderer,
        old: &Scene,
        new: &Scene,
    ) -> Result<Damage, Undrawable> {
        renderer.update(self, old, new)
    }
}

impl Renderer {
    /// The scene laid out once and made into its display list in `theme` —
    /// every color its value there — or why it can't be drawn: images and
    /// shadows that don't fit the GPU's atlas together. What the PNG, the
    /// canvas and `devpage check` all start from.
    ///
    /// Each image is painted under a blob of its own, made in the order the
    /// drawing first paints it — except that a drawing made `after` the one
    /// a canvas last showed, painting the same images in the same order,
    /// paints that drawing's: the GPU then keeps the atlas it has, which
    /// holds exactly them, placed as a fresh one would.
    pub fn drawing(
        &mut self,
        scene: &Scene,
        theme: Theme,
        after: Option<&Drawing>,
    ) -> Result<Drawing, Undrawable> {
        self.register(&scene.fonts);
        let mut tree: TaffyTree<Option<Leaf>> = TaffyTree::new();
        let mut hugs = Vec::new();
        let root = build(&mut tree, &scene.root, None, &mut hugs, &scene.images);
        self.settle(&mut tree, root, &hugs);
        let frames = frames_by_order(&tree, root, &scene.root);
        let mut d = Drawing {
            tree,
            root,
            theme,
            frames: BTreeMap::new(),
            recs: BTreeMap::new(),
            list: List::new(),
            blobs: HashMap::new(),
            images: Vec::new(),
            shows: Vec::new(),
        };
        self.blobs.clear();
        let mut made = Vec::new();
        let (t, c) = (d.list.root_transform, d.list.root_clip);
        let mut walk = Walk {
            scene,
            frames: &frames,
            dirty: &|_| true,
            damage: None,
            fresh: Some(&mut made),
        };
        self.sync(
            &mut d,
            &mut walk,
            &scene.root,
            vec![0],
            t,
            Vec2::ZERO,
            c,
            None,
            None,
            false,
        );
        d.list.load(made);
        d.frames = frames;
        d.blobs = std::mem::take(&mut self.blobs);
        self.settle_area(&mut d);
        self.settle_images(&mut d)?;
        if let Some(last) = after
            && last.shows.len() == d.shows.len()
            && last.shows.iter().zip(&d.shows).all(|(a, b)| a.same(b))
        {
            let last_of: HashMap<u64, ImageData> = d
                .shows
                .iter()
                .zip(&last.shows)
                .map(|(new, old)| (new.data.data.id(), old.data.clone()))
                .collect();
            d.list.swap_images(&last_of);
            for rec in d.recs.values_mut() {
                for (p, _) in &mut rec.painted {
                    if let Some(old) = last_of.get(&p.data.data.id()) {
                        p.data = old.clone();
                    }
                }
            }
            for data in d.blobs.values_mut() {
                if let Some(old) = last_of.get(&data.data.id()) {
                    *data = old.clone();
                }
            }
            d.shows = last.shows.clone();
            d.images = d.shows.iter().map(|p| p.data.clone()).collect();
        }
        Ok(d)
    }

    fn update(&mut self, d: &mut Drawing, old: &Scene, new: &Scene) -> Result<Damage, Undrawable> {
        self.register(&new.fonts);
        let replaced: Vec<Order> = crate::wire::diff(&old.root, &new.root)
            .into_iter()
            .map(|r| r.id.split('.').map(|s| s.parse().unwrap_or(0)).collect())
            .collect();
        for order in &replaced {
            let node = find_node(&new.root, order);
            let parent = (order.len() > 1).then(|| find_node(&new.root, &order[..order.len() - 1]));
            let mut hugs = Vec::new();
            let id = build(&mut d.tree, node, parent, &mut hugs, &new.images);
            let gone = if order.len() == 1 {
                std::mem::replace(&mut d.root, id)
            } else {
                let parent_id = taffy_at(&d.tree, d.root, &order[1..order.len() - 1]);
                d.tree
                    .replace_child_at_index(parent_id, order[order.len() - 1] as usize, id)
                    .expect("child")
            };
            remove_subtree(&mut d.tree, gone);
        }
        let mut hugs = Vec::new();
        collect_hugs(&d.tree, d.root, &new.root, None, &mut hugs);
        let root = d.root;
        self.settle(&mut d.tree, root, &hugs);
        let frames = frames_by_order(&d.tree, d.root, &new.root);
        let mut damage = Damage::default();
        // The chunks and property-tree nodes of nodes that are gone.
        let gone: Vec<Order> = d
            .frames
            .keys()
            .filter(|o| !frames.contains_key(*o))
            .cloned()
            .collect();
        for o in gone {
            if let Some(c) = d.list.take(&o) {
                damage.rects.extend(c.bounds);
                d.list.free(&c.made);
            }
            if let Some(rec) = d.recs.remove(&o) {
                d.list.trees.transforms.remove(rec.transform);
                if let Some(e) = rec.effect {
                    d.list.trees.effects.remove(e);
                }
                if let Some(c) = rec.clip {
                    d.list.trees.clips.remove(c);
                }
            }
        }
        let dirty = |o: &Order| replaced.iter().any(|r| o.starts_with(r));
        self.blobs = std::mem::take(&mut d.blobs);
        let (t, c) = (d.list.root_transform, d.list.root_clip);
        let mut walk = Walk {
            scene: new,
            frames: &frames,
            dirty: &dirty,
            damage: Some(&mut damage),
            fresh: None,
        };
        self.sync(
            d,
            &mut walk,
            &new.root,
            vec![0],
            t,
            Vec2::ZERO,
            c,
            None,
            None,
            false,
        );
        d.frames = frames;
        d.blobs = std::mem::take(&mut self.blobs);
        let area = d.list.area;
        self.settle_area(d);
        let now = d.list.area;
        if now != area {
            if (now.x, now.y) != (area.x, area.y) {
                damage.all = true;
            } else {
                // What the area gained or lost along its right and bottom.
                let (w0, w1) = (area.width.min(now.width), area.width.max(now.width));
                let (h0, h1) = (area.height.min(now.height), area.height.max(now.height));
                let (x, y) = (now.x as f64, now.y as f64);
                if w0 < w1 {
                    damage
                        .rects
                        .push(KRect::new(x + w0 as f64, y, x + w1 as f64, y + h1 as f64));
                }
                if h0 < h1 {
                    damage
                        .rects
                        .push(KRect::new(x, y + h0 as f64, x + w1 as f64, y + h1 as f64));
                }
            }
        }
        self.settle_images(d)?;
        Ok(damage)
    }

    /// The drawing's area from its root's box and what its chunks cover,
    /// rounded out to whole pixels, ignoring the slivers floating point
    /// leaves on a curve's bounds.
    fn settle_area(&mut self, d: &mut Drawing) {
        let root = d.frames[&vec![0]];
        let mut area = frame_rect(root);
        if let Some(covered) = d.list.covered() {
            area = area.union(covered);
        }
        const SLIVER: f64 = 0.01;
        let area = KRect::new(
            (area.x0 + SLIVER).floor(),
            (area.y0 + SLIVER).floor(),
            (area.x1 - SLIVER).ceil(),
            (area.y1 - SLIVER).ceil(),
        );
        d.list.set_area(Frame {
            x: area.x0 as f32,
            y: area.y0 as f32,
            width: area.width() as f32,
            height: area.height() as f32,
        });
    }

    /// The images the drawing paints, in the order it first paints them —
    /// each image file once — and whether they fit the GPU's atlas together.
    fn settle_images(&mut self, d: &mut Drawing) -> Result<(), Undrawable> {
        let mut files = BTreeSet::new();
        let mut painted: Vec<(Painted, Origin)> = Vec::new();
        for rec in d.recs.values() {
            for (p, at) in &rec.painted {
                if p.file.is_none_or(|id| files.insert(id)) {
                    painted.push((p.clone(), at.clone()));
                }
            }
        }
        d.blobs.retain(|id, _| files.contains(id));
        d.shows = painted.iter().map(|(p, _)| p.clone()).collect();
        d.images = d.shows.iter().map(|p| p.data.clone()).collect();
        let sizes: Vec<(u32, u32)> = painted
            .iter()
            .map(|(p, _)| (p.data.width, p.data.height))
            .collect();
        if let Err(n) = crate::atlas::place(&sizes) {
            let (Painted { data, .. }, at) = &painted[n];
            let what = if at.file.is_empty() {
                "an image"
            } else {
                "this"
            };
            return Err(Undrawable {
                message: format!(
                    "{what} doesn't fit: the images a drawing paints — its image files, \
                     at most {side} pixels a side, and its shadows — share the GPU's one \
                     {atlas}×{atlas} atlas, and the {n} before this {w}×{h} fill it",
                    side = resolve::MAX_IMAGE_SIDE,
                    atlas = crate::atlas::SIDE,
                    w = data.width,
                    h = data.height,
                ),
                at: Some(at.clone()),
            });
        }
        Ok(())
    }

    /// Makes `node`'s chunk again when it is dirty, new or another size, or
    /// moves it when only its place changed; then its children's. `remade`
    /// says its parent's clip or effect was made anew, which its own
    /// commands are under.
    #[allow(clippy::too_many_arguments)]
    fn sync(
        &mut self,
        d: &mut Drawing,
        w: &mut Walk,
        node: &Node,
        order: Order,
        parent_t: Id,
        parent_origin: Vec2,
        clip: Id,
        cut: Option<KRect>,
        effect: Option<Id>,
        remade: bool,
    ) {
        let f = w.frames[&order];
        let origin = Vec2::new(f.x as f64, f.y as f64);
        let old = d.recs.remove(&order);
        let resized = d
            .frames
            .get(&order)
            .is_none_or(|was| (was.width, was.height) != (f.width, f.height));
        let remake = old.is_none() || remade || (w.dirty)(&order) || resized;
        let trees = &mut d.list.trees;
        // Its transform is kept, and moved: its children's are under it.
        let transform = match &old {
            Some(rec) => {
                trees.transforms.get_mut(rec.transform).offset = origin - parent_origin;
                rec.transform
            }
            None => trees.transforms.add(Transform {
                parent: Some(parent_t),
                offset: origin - parent_origin,
            }),
        };
        let mut children_remade = remade;
        let (own_effect, own_clip) = if remake {
            // Its opacity and its clip are changed in place where they stay,
            // so what its children paint under them still names them.
            let opacity = (node.opacity < 1.0).then_some(node.opacity);
            let had = old.as_ref().and_then(|r| r.effect);
            let new_effect = match (had, opacity) {
                (Some(e), Some(o)) if !remade => {
                    trees.effects.get_mut(e).blend = Blend::Opacity(o);
                    Some(e)
                }
                (had, o) => {
                    if let Some(e) = had {
                        trees.effects.remove(e);
                    }
                    children_remade |= had.is_some() || o.is_some();
                    o.map(|o| {
                        trees.effects.add(Effect {
                            parent: effect,
                            clip,
                            transform,
                            blend: Blend::Opacity(o),
                            bounds: None,
                        })
                    })
                }
            };
            let shape = node.clip.then(|| {
                node_shape(
                    node,
                    KRect::new(0.0, 0.0, f.width as f64, f.height as f64),
                    0.0,
                )
            });
            let had = old.as_ref().and_then(|r| r.clip);
            let new_clip = match (had, shape) {
                (Some(c), Some(path)) if !remade => {
                    trees.clips.get_mut(c).path = path;
                    Some(c)
                }
                (had, shape) => {
                    if let Some(c) = had {
                        trees.clips.remove(c);
                    }
                    children_remade |= had.is_some() || shape.is_some();
                    shape.map(|path| {
                        trees.clips.add(Clip {
                            parent: Some(clip),
                            transform,
                            path,
                        })
                    })
                }
            };
            (new_effect, new_clip)
        } else {
            let rec = old.as_ref().expect("kept");
            (rec.effect, rec.clip)
        };
        let under = own_effect.or(effect);
        let was = d.list.take(&order);
        let (chunk, painted) = match was {
            Some(c) if !remake => {
                let moved = c.origin != origin || c.cut != cut;
                if moved && let Some(damage) = w.damage.as_deref_mut() {
                    damage.rects.extend(c.bounds);
                }
                let c = c.moved(origin, cut);
                if moved && let Some(damage) = w.damage.as_deref_mut() {
                    damage.rects.extend(c.bounds);
                }
                (c, old.expect("kept").painted)
            }
            was => {
                if let Some(c) = &was {
                    if let Some(damage) = w.damage.as_deref_mut() {
                        damage.rects.extend(c.bounds);
                    }
                    d.list.free(&c.made);
                }
                let local = Frame {
                    x: 0.0,
                    y: 0.0,
                    width: f.width,
                    height: f.height,
                };
                self.painted.clear();
                let mut covers = Vec::new();
                let mut b = Builder::new(&mut d.list.trees, transform, clip, under);
                self.own(w.scene, node, local, d.theme, &mut b, &mut covers);
                let mut built = b.finish();
                let mut painted = std::mem::take(&mut self.painted);
                // A shadow that paints the very pixels it painted before
                // keeps its blobs, so the GPU keeps its atlas.
                if let Some(rec) = &old
                    && rec.painted.len() == painted.len()
                    && rec
                        .painted
                        .iter()
                        .zip(&painted)
                        .all(|(a, b)| a.0.same(&b.0))
                {
                    let last_of: HashMap<u64, ImageData> = painted
                        .iter()
                        .zip(&rec.painted)
                        .map(|(new, old)| (new.0.data.data.id(), old.0.data.clone()))
                        .collect();
                    swap(&mut built.0, &last_of);
                    painted = rec.painted.clone();
                }
                let action = node
                    .on_click
                    .clone()
                    .map(|a| (a, KRect::new(0.0, 0.0, f.width as f64, f.height as f64)));
                let c = Chunk::new(transform, origin, built, covers, action, cut);
                if let Some(damage) = w.damage.as_deref_mut() {
                    damage.rects.extend(c.bounds);
                }
                (c, painted)
            }
        };
        d.recs.insert(
            order.clone(),
            Rec {
                transform,
                effect: own_effect,
                clip: own_clip,
                painted,
            },
        );
        match w.fresh.as_deref_mut() {
            Some(fresh) => fresh.push((order.clone(), chunk)),
            None => d.list.put(order.clone(), chunk),
        }
        let rect = frame_rect(f);
        let (inner, inner_cut) = match own_clip {
            Some(c) => (c, Some(cut.map_or(rect, |k| rect.intersect(k)))),
            None => (clip, cut),
        };
        for (i, child) in node.children.iter().enumerate() {
            let mut o = order.clone();
            o.push(i as u32);
            if !w.frames.contains_key(&o) {
                continue;
            }
            self.sync(
                d,
                w,
                child,
                o,
                transform,
                origin,
                inner,
                inner_cut,
                under,
                children_remade,
            );
        }
    }
}

/// What a [`Renderer::sync`] walk reads and adds to.
struct Walk<'a> {
    scene: &'a Scene,
    frames: &'a BTreeMap<Order, Frame>,
    dirty: &'a dyn Fn(&Order) -> bool,
    damage: Option<&'a mut Damage>,
    /// For a new drawing, the chunks made, to bulk-load the index with.
    fresh: Option<&'a mut Vec<(Order, Chunk)>>,
}

/// The commands' images swapped for the blobs `last_of` names.
fn swap(commands: &mut [crate::display::Command], last_of: &HashMap<u64, ImageData>) {
    for c in commands {
        if let Draw::Fill(_, Paint::Image(data, _)) = &mut c.draw
            && let Some(old) = last_of.get(&data.data.id())
        {
            *data = old.clone();
        }
    }
}

/// Removes `id` and everything under it from the layout tree.
fn remove_subtree(tree: &mut TaffyTree<Option<Leaf>>, id: NodeId) {
    for child in tree.children(id).unwrap_or_default() {
        remove_subtree(tree, child);
    }
    let _ = tree.remove(id);
}

impl Renderer {
    /// Lays the tree out, hugging no-layout frames to their children.
    fn settle(
        &mut self,
        tree: &mut TaffyTree<Option<Leaf>>,
        root: NodeId,
        hugs: &[(NodeId, bool, bool)],
    ) {
        for _ in 0..=hugs.len() {
            self.compute(tree, root);
            let mut changed = false;
            for &(id, w, h) in hugs {
                let (mut ew, mut eh) = (0.0f32, 0.0f32);
                for child in tree.children(id).expect("children") {
                    let l = tree.layout(child).expect("laid out");
                    ew = ew.max(l.location.x + l.size.width);
                    eh = eh.max(l.location.y + l.size.height);
                }
                let mut style = tree.style(id).expect("style").clone();
                let mut ch = false;
                let mut set = |d: &mut Dimension, v: f32| {
                    if *d != Dimension::length(v) {
                        *d = Dimension::length(v);
                        ch = true;
                    }
                };
                if w {
                    set(&mut style.size.width, ew);
                }
                if h {
                    set(&mut style.size.height, eh);
                }
                if ch {
                    tree.set_style(id, style).expect("style");
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }
}

fn frame_rect(f: Frame) -> KRect {
    KRect::new(
        f.x as f64,
        f.y as f64,
        (f.x + f.width) as f64,
        (f.y + f.height) as f64,
    )
}

fn find_node<'a>(root: &'a Node, order: &[u32]) -> &'a Node {
    let mut at = root;
    for &i in &order[1..] {
        at = &at.children[i as usize];
    }
    at
}

fn taffy_at(tree: &TaffyTree<Option<Leaf>>, root: NodeId, path: &[u32]) -> NodeId {
    let mut at = root;
    for &i in path {
        at = tree.child_at_index(at, i as usize).expect("child");
    }
    at
}

fn collect_hugs(
    tree: &TaffyTree<Option<Leaf>>,
    id: NodeId,
    node: &Node,
    parent: Option<&Node>,
    out: &mut Vec<(NodeId, bool, bool)>,
) {
    if node.text.is_some() || node.vector.is_some() || node.image.is_some() {
        return;
    }
    for (i, c) in node.children.iter().enumerate() {
        collect_hugs(
            tree,
            tree.child_at_index(id, i).expect("child"),
            c,
            Some(node),
            out,
        );
    }
    let hugs_by = |s: DSize| s == DSize::Hug || (parent.is_none() && s == DSize::Fill);
    let (w, h) = (hugs_by(node.width), hugs_by(node.height));
    if node.layout == Layout::None && !node.children.is_empty() && (w || h) {
        out.push((id, w, h));
    }
}

fn frames_by_order(
    tree: &TaffyTree<Option<Leaf>>,
    root: NodeId,
    node: &Node,
) -> std::collections::BTreeMap<Vec<u32>, Frame> {
    let mut out = std::collections::BTreeMap::new();
    fn go(
        tree: &TaffyTree<Option<Leaf>>,
        id: NodeId,
        node: &Node,
        order: Vec<u32>,
        ox: f32,
        oy: f32,
        out: &mut std::collections::BTreeMap<Vec<u32>, Frame>,
    ) {
        let l = tree.layout(id).expect("laid out");
        let (x, y) = (ox + l.location.x, oy + l.location.y);
        out.insert(
            order.clone(),
            Frame {
                x,
                y,
                width: l.size.width,
                height: l.size.height,
            },
        );
        for (i, c) in node.children.iter().enumerate() {
            let mut o = order.clone();
            o.push(i as u32);
            go(
                tree,
                tree.child_at_index(id, i).expect("child"),
                c,
                o,
                x,
                y,
                out,
            );
        }
    }
    go(tree, root, node, vec![0], 0.0, 0.0, &mut out);
    out
}

/// A node's box shape at `rect`, grown by `-inset`.
fn node_shape(node: &Node, rect: KRect, inset: f64) -> BezPath {
    let r = rect.inset(-inset);
    if node.kind == Kind::Ellipse {
        KEllipse::from_rect(r).to_path(0.1)
    } else {
        let [tl, tr, br, bl] = node.radius.map(|v| (v as f64 - inset).max(0.0));
        RoundedRect::from_rect(r, (tl, tr, br, bl)).to_path(0.1)
    }
}

/// The `vello_cpu` backend of [`crate::display::paint`].
pub struct Cpu<'a> {
    pub ctx: &'a mut RenderContext,
    pub sources: HashMap<u64, vello_cpu::ImageSource>,
    pub resources: Resources,
}

impl crate::display::Target for Cpu<'_> {
    fn draw(&mut self, draw: &Draw, t: Affine) {
        let ctx = &mut *self.ctx;
        ctx.set_transform(t);
        match draw {
            Draw::Fill(path, paint) => {
                match paint {
                    Paint::Solid(c) => ctx.set_paint(*c),
                    Paint::Gradient(g) => ctx.set_paint(g.clone()),
                    Paint::Image(data, place) => {
                        let image = self
                            .sources
                            .entry(data.data.id())
                            .or_insert_with(|| vello_cpu::ImageSource::from_peniko_image_data(data))
                            .clone();
                        ctx.set_paint(ImageBrush {
                            image,
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
            Draw::Glyphs(font, size, c, coords, glyphs) => {
                ctx.set_paint(*c);
                let _ = ctx
                    .glyph_run(&mut self.resources, font)
                    .font_size(*size)
                    .normalized_coords(coords)
                    .hint(true)
                    .fill_glyphs(glyphs.iter().copied());
            }
        }
    }
    fn push_clip(&mut self, path: &BezPath, t: Affine) {
        self.ctx.set_transform(t);
        self.ctx.push_clip_layer(path);
    }
    fn push_layer(
        &mut self,
        blend: crate::display::Blend,
        bounds: Option<&BezPath>,
        t: Affine,
        area: KRect,
    ) {
        let whole;
        let bounds = match bounds {
            Some(b) => {
                self.ctx.set_transform(t);
                b
            }
            None => {
                self.ctx.set_transform(Affine::IDENTITY);
                whole = area.to_path(0.1);
                &whole
            }
        };
        match blend {
            crate::display::Blend::Opacity(o) => {
                self.ctx.push_layer(Some(bounds), None, Some(o), None, None)
            }
            crate::display::Blend::Add => self.ctx.push_layer(
                Some(bounds),
                Some(BlendMode::new(Mix::Normal, Compose::Plus)),
                None,
                None,
                None,
            ),
        }
    }
    fn pop(&mut self) {
        self.ctx.pop_layer();
    }
}

/// Paints `area` of `list` — in the design's coordinates, whole pixels at
/// `scale` — with `vello_cpu`: one tile, band or whole drawing.
pub fn paint_area(list: &crate::display::List, area: KRect, scale: f64) -> Pixmap {
    // Whole pixels, the last partly covered one included — but not one a
    // rounding error leaves past a tile's exact edge.
    let side = |v: f64| ((v * scale - 1e-6).ceil().clamp(1.0, u16::MAX as f64)) as u16;
    let (w, h) = (side(area.width()), side(area.height()));
    let mut ctx = RenderContext::new(w, h);
    let mut cpu = Cpu {
        ctx: &mut ctx,
        sources: HashMap::new(),
        resources: Resources::default(),
    };
    crate::display::paint(list, area, scale, &mut cpu);
    let mut resources = std::mem::take(&mut cpu.resources);
    ctx.flush();
    let mut pixmap = Pixmap::new(w, h);
    ctx.render(&mut pixmap, &mut resources);
    pixmap
}

/// What a leaf of the layout tree measures by: a text, or a vector's or an
/// image's own size.
enum Leaf {
    /// A text, and whether it wraps at its width.
    Text(Box<resolve::TextStyle>, bool),
    Intrinsic(f32, f32),
}

fn color(c: Rgba) -> PColor {
    PColor::from_rgba8(c.r, c.g, c.b, c.a)
}

/// A CSS linear gradient over `rect`: 0° points up, 90° right; the line runs
/// through the center, long enough that its ends touch the far corners.
/// Its stops take their colors in `theme`.
fn gradient(rect: KRect, angle: f32, stops: &[(Color, f32)], theme: Theme) -> Gradient {
    let a = (angle as f64).to_radians();
    let dir = Vec2::new(a.sin(), -a.cos());
    let half = (rect.width() * a.sin().abs() + rect.height() * a.cos().abs()) / 2.0;
    let center = rect.center();
    let stops: Vec<(f32, PColor)> = stops
        .iter()
        .map(|(c, at)| (*at, color(c.get(theme))))
        .collect();
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
    hugs: &mut Vec<(NodeId, bool, bool)>,
    images: &ImageFiles,
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
        return tree
            .new_leaf_with_context(style, Some(Leaf::Text(Box::new(text.clone()), wraps)))
            .expect("leaf");
    }
    let intrinsic = match (&node.vector, &node.image) {
        (Some(v), _) => Some((v.view_box[2], v.view_box[3])),
        (_, Some(i)) => images.get(&i.id).map(|f| (f.width as f32, f.height as f32)),
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
        .map(|c| build(tree, c, Some(node), hugs, images))
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

#[cfg(test)]
impl Renderer {
    /// The scene's whole drawing painted with `vello_cpu` at `scale`.
    pub fn paint(&mut self, scene: &Scene, theme: Theme, scale: f32) -> Pixmap {
        let d = self.drawing(scene, theme, None).expect("drawable");
        paint_area(&d.list, frame_rect(d.area()), scale as f64)
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

    /// The repository's files a design can set text in, and one design.
    struct Fonts(HashMap<String, Vec<u8>>);

    impl crate::Files for Fonts {
        fn bytes(&self, path: &str) -> Option<Vec<u8>> {
            self.0.get(path).cloned()
        }
        fn list(&self, path: &str) -> Option<Vec<String>> {
            resolve::listed(self.0.keys(), path)
        }
    }

    fn with_fonts(source: &str) -> Scene {
        let root = env!("CARGO_MANIFEST_DIR");
        let read = |p: String| std::fs::read(p).unwrap();
        let files = Fonts(
            [
                (
                    "v.ttf".to_string(),
                    read(format!(
                        "{root}/../docs/designs/checkout/fonts/Figtree/Figtree-Variable.ttf"
                    )),
                ),
                (
                    "f/Inter-Regular.ttf".to_string(),
                    read(format!("{root}/fonts/Inter-Regular.ttf")),
                ),
                (
                    "themes.yml".to_string(),
                    b"kind: themes\nfonts:\n  fig: ./v.ttf\n  inter: ./f\nlight: {}\ndark: {}\n"
                        .to_vec(),
                ),
                ("a.design".to_string(), source.as_bytes().to_vec()),
            ]
            .into(),
        );
        resolve::resolve(&files, "a.design").unwrap()
    }

    /// A text is measured in its font, and a variable font at its weight:
    /// its glyph runs carry the point on the weight axis, which a static
    /// font's don't.
    #[test]
    fn a_text_is_laid_out_in_its_font_at_its_weight() {
        let line = |font: &str, weight: u16| {
            let s = with_fonts(&format!(
                r#"<Frame><Text size={{40}} weight={{{weight}}}{font}>Mmmmm wwwww</Text></Frame>"#
            ));
            let mut r = Renderer::new();
            let width = r.layout(&s)[1].width;
            let d = r.drawing(&s, Theme::Light, None).unwrap();
            let coords: Vec<Vec<i16>> = d
                .list
                .chunks()
                .flat_map(|(_, c)| c.commands.iter().map(|c| &c.draw))
                .filter_map(|d| match d {
                    Draw::Glyphs(_, _, _, coords, _) => Some(coords.clone()),
                    _ => None,
                })
                .collect();
            (width, coords)
        };
        let (inter, coords) = line("", 400);
        assert!(coords.iter().all(|c| c.is_empty()));
        let (same, _) = line(r#" font="inter""#, 400);
        assert_eq!(
            same, inter,
            "Inter from a folder measures as the embedded one"
        );
        let (light, light_coords) = line(r#" font="fig""#, 400);
        let (heavy, heavy_coords) = line(r#" font="fig""#, 900);
        assert_ne!(light, inter);
        assert!(heavy > light, "{heavy} ≤ {light}");
        assert!(!heavy_coords.is_empty() && heavy_coords.iter().all(|c| !c.is_empty()));
        assert_ne!(light_coords, heavy_coords);
    }

    /// A renderer that laid out scenes in other fonts lays a scene out as a
    /// fresh one does, and holds no more than one scene's font files.
    #[test]
    fn a_renderer_lays_out_as_a_fresh_one_whatever_it_drew_before() {
        let a = with_fonts(
            r#"<Frame width={120} layout="column"><Text width="fill" font="fig" weight={700}>Washed linen with a relaxed fit</Text></Frame>"#,
        );
        let b = with_fonts(
            r#"<Frame width={120} layout="column"><Text width="fill" font="inter">Washed linen with a relaxed fit</Text></Frame>"#,
        );
        let mut used = Renderer::new();
        for s in [&a, &b, &a, &b] {
            assert_eq!(used.layout(s), Renderer::new().layout(s));
            assert_eq!(used.registered.as_ref().unwrap().len(), 1);
        }
        let inter = scene(
            r#"<Frame width={120} layout="column"><Text width="fill">Washed linen with a relaxed fit</Text></Frame>"#,
        );
        assert_eq!(used.layout(&inter), Renderer::new().layout(&inter));
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
        let p = Renderer::new().paint(&s, Theme::Light, 1.0);
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
        let p = Renderer::new().paint(&s, Theme::Light, 1.0);
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
        let p = Renderer::new().paint(&down, Theme::Light, 1.0);
        assert!(close(rgba(&p, 10, 0), [255, 0, 0, 255]));
        assert!(close(rgba(&p, 10, 99), [0, 0, 255, 255]));
    }

    #[test]
    fn a_stroke_is_drawn_inside_the_box() {
        let s = scene(
            r##"<Frame width={40} height={40}><Rect x={10} y={10} width={20} height={20} stroke="#ff0000" strokeWidth={4} /></Frame>"##,
        );
        let p = Renderer::new().paint(&s, Theme::Light, 1.0);
        assert!(close(rgba(&p, 11, 20), [255, 0, 0, 255]));
        assert!(close(rgba(&p, 13, 20), [255, 0, 0, 255]));
        assert_eq!(rgba(&p, 9, 20)[3], 0, "nothing outside the box");
        assert_eq!(rgba(&p, 20, 20)[3], 0, "nothing in the middle");
    }

    #[test]
    fn each_corner_takes_its_own_radius() {
        let s =
            scene(r##"<Frame width={40} height={40} radius={[20, 0, 0, 0]} fill="#000000" />"##);
        let p = Renderer::new().paint(&s, Theme::Light, 1.0);
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
        let p = Renderer::new().paint(&sharp, Theme::Light, 1.0);
        assert_eq!(rgba(&p, 57, 7)[3], 0, "top left is rounded by 15");
        assert_eq!(rgba(&p, 92, 7)[3], 255, "top right only by 5");
        assert_eq!(rgba(&p, 92, 42)[3], 255, "bottom right only by 5");
        assert_eq!(rgba(&p, 57, 42)[3], 255, "bottom left only by 5");
        assert_eq!(rgba(&p, 94, 5)[3], 0, "but rounded");
        // Blurred, each corner still fades by its own radius.
        let blurred = scene(
            r##"<Frame width={100} height={60}><Rect x={10} y={10} width={30} height={30} radius={[20, 0, 0, 0]} shadow={{ x: 50, blur: 6, color: "#000000" }} /></Frame>"##,
        );
        let p = Renderer::new().paint(&blurred, Theme::Light, 1.0);
        let (tl, tr) = (rgba(&p, 63, 13)[3], rgba(&p, 86, 13)[3]);
        assert!(tr > 150 && tl < 100, "top left {tl}, top right {tr}");
        let (bl, br) = (rgba(&p, 63, 36)[3], rgba(&p, 86, 36)[3]);
        assert!(
            bl.abs_diff(tr) <= 2 && br.abs_diff(tr) <= 2,
            "{bl} {br} {tr}"
        );
    }

    /// The alpha across a box's left edge, a pixel at a time, through the
    /// middle of a `size` square box with a black shadow blurred by `blur`.
    /// Only a strip around the edge is painted, so even the largest box is
    /// cheap to look at.
    fn shadow_edge(size: u32, blur: f32) -> Vec<u8> {
        let s = scene(&format!(
            r##"<Frame width={{{}}} height={{{}}}><Rect x={{40}} y={{40}} width={{{size}}} height={{{size}}} shadow={{{{ blur: {blur}, color: "#000000" }}}} /></Frame>"##,
            size + 80,
            size + 80
        ));
        let mut r = Renderer::new();
        let d = r.drawing(&s, Theme::Light, None).unwrap();
        let middle = 40.0 + size as f32 / 2.0;
        let strip = Frame {
            x: 0.0,
            y: middle.floor(),
            width: 80.0,
            height: 1.0,
        };
        let p = paint_area(&d.list, frame_rect(strip), 1.0);
        (0..80).map(|x| p.sample(x, 0).a).collect()
    }

    /// Where a rising run of alphas crosses `level`, in pixels, between pixel
    /// centers.
    fn crossing(alpha: &[u8], level: f32) -> f32 {
        let i = alpha.iter().position(|&a| a as f32 >= level).unwrap();
        let (a, b) = (alpha[i - 1] as f32, alpha[i] as f32);
        i as f32 - 1.0 + (level - a) / (b - a) + 0.5
    }

    /// A shadow is a true Gaussian at every allowed size: it fades to nothing
    /// three standard deviations out, and its edge rises from 10% to 90% over
    /// 2.563 standard deviations — however large the box, however slight the
    /// blur.
    #[test]
    fn a_shadow_edge_is_the_gaussian_at_every_size() {
        for size in [100, 2000, 8000, 16000] {
            for blur in [2.0f32, 8.0] {
                let sigma = blur / 2.0;
                let alpha = shadow_edge(size, blur);
                // The box's edge is at x = 40. Two and a half standard
                // deviations out a Gaussian edge is 0.6% (1.6 of 255), and
                // past that less: no step where the shadow's image ends.
                let reach = (39.5 - 2.5 * sigma).floor() as usize + 1;
                assert!(
                    alpha[..reach].iter().all(|&a| a <= 2),
                    "box {size} blur {blur}: {:?}",
                    &alpha[..48]
                );
                let ramp = crossing(&alpha, 0.9 * 255.0) - crossing(&alpha, 0.1 * 255.0);
                assert!(
                    (ramp / sigma - 2.563).abs() < 0.2,
                    "box {size} blur {blur}: 10–90% over {ramp} px, σ {sigma}: {:?}",
                    &alpha[..48]
                );
                let half = crossing(&alpha, 0.5 * 255.0);
                assert!(
                    (half - 40.0).abs() < 0.25,
                    "box {size} blur {blur}: half way at {half}"
                );
            }
        }
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
        let p = Renderer::new().paint(&s, Theme::Light, 1.0);
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
        let p = Renderer::new().paint(&s, Theme::Light, 1.0);
        assert_eq!(rgba(&p, 15, 15)[3], 255);
        assert_eq!(rgba(&p, 25, 25)[3], 0);
    }

    #[test]
    fn a_vector_fills_its_path_scaled_into_its_box() {
        // A triangle in a 10×10 view box, drawn at 40×40.
        let s = scene(
            r##"<Frame width={40} height={40}><Vector d="M0 0L10 0L0 10Z" viewBox="0 0 10 10" width={40} height={40} fill="#000000" /></Frame>"##,
        );
        let p = Renderer::new().paint(&s, Theme::Light, 1.0);
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
        let file = std::sync::Arc::new(resolve::ImageFile::new(image).unwrap());
        s.root.image = Some(resolve::Image {
            src: "i.png".into(),
            id: file.id,
            at: Default::default(),
        });
        s.images.insert(file.id, file);
        // The image is sampled smoothly, so each side is mostly its color.
        let p = Renderer::new().paint(&s, Theme::Light, 1.0);
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

    /// A drawing is made for a theme: each color its value there.
    #[test]
    fn a_drawing_takes_each_colors_value_in_its_theme() {
        let files: HashMap<String, String> = [
            (
                "themes.yml".to_string(),
                "kind: themes\nlight:\n  bg: \"#ffffff\"\n  edge: \"#ff0000\"\ndark:\n  bg: \"#000000\"\n  edge: \"#00ff00\"\n"
                    .to_string(),
            ),
            (
                "a.design".to_string(),
                r##"<Frame width={40} height={20} fill={theme.bg} stroke={theme.edge} strokeWidth={4}>
  <Rect x={10} y={8} width={20} height={4} fill="linear-gradient(90deg, theme.edge 0%, theme.edge 100%)" />
</Frame>"##
                    .to_string(),
            ),
        ]
        .into();
        let s = resolve::resolve(&files, "a.design").unwrap();
        let mut r = Renderer::new();
        for (theme, bg, edge) in [
            (Theme::Light, [255, 255, 255, 255], [255, 0, 0, 255]),
            (Theme::Dark, [0, 0, 0, 255], [0, 255, 0, 255]),
        ] {
            let p = r.paint(&s, theme, 1.0);
            assert_eq!(rgba(&p, 6, 6), bg, "{theme:?}");
            assert_eq!(rgba(&p, 1, 1), edge, "{theme:?}");
            assert_eq!(rgba(&p, 20, 10), edge, "{theme:?}");
        }
    }

    #[test]
    fn paints_fills_and_text() {
        let s = scene(
            r##"<Frame width={100} height={40} fill="#ff0000" layout="row" padding={8}><Text color="#ffffff">Pay</Text></Frame>"##,
        );
        let pixmap = Renderer::new().paint(&s, Theme::Light, 1.0);
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
        let area = r.drawing(&s, Theme::Light, None).unwrap().area();
        assert_eq!(
            area,
            Frame {
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 30.0
            }
        );
        assert_eq!(r.paint(&s, Theme::Light, 1.0).width(), 40);
        // A shadow and a child past the edge widen it; the PNG shows them.
        let s = scene(
            r##"<Frame width={40} height={30} fill="#ffffff" shadow={{ y: 4, blur: 8, color: "#000000ff" }}>
  <Rect x={30} y={-10} width={20} height={10} fill="#ff0000" />
</Frame>"##,
        );
        let area = r.drawing(&s, Theme::Light, None).unwrap().area();
        assert_eq!(
            area,
            Frame {
                x: -12.0,
                y: -10.0,
                width: 64.0,
                height: 56.0
            }
        );
        let p = r.paint(&s, Theme::Light, 1.0);
        assert_eq!((p.width(), p.height()), (64, 56));
        // The red child, at (30..50, -10..0), drawn at its place in the PNG.
        assert!(close(rgba(&p, 12 + 45, 5), [255, 0, 0, 255]));
        // The shadow, below the box.
        assert!(rgba(&p, 12 + 20, 10 + 30 + 3)[3] > 100);
        // A clipping parent cuts what its children paint, and the area too.
        let s = scene(
            r##"<Frame width={40} height={30} clip><Rect x={30} width={50} height={10} fill="#ff0000" /></Frame>"##,
        );
        assert_eq!(
            r.drawing(&s, Theme::Light, None).unwrap().area().width,
            40.0
        );
    }

    #[test]
    fn a_drawing_of_any_size_is_a_picture() {
        let mut r = Renderer::new();
        let s = scene(r##"<Frame width={70000} height={10} fill="#000000" />"##);
        let (px, w, h) = pixels(&crate::png(&mut r, &s, Theme::Light).unwrap()).unwrap();
        assert_eq!((w, h), (70000, 10));
        assert_eq!(&px[px.len() - 4..], &[0, 0, 0, 255]);
    }

    /// Files whose `.png`s are image files of the size their name says,
    /// `w`x`h`.png — made without decoding, their pixels never touched.
    struct Sized(HashMap<String, String>);

    impl crate::Files for Sized {
        fn bytes(&self, path: &str) -> Option<Vec<u8>> {
            self.0.get(path).map(|s| s.clone().into_bytes())
        }
        fn list(&self, path: &str) -> Option<Vec<String>> {
            self.0.list(path)
        }
        fn image(&self, path: &str) -> Option<Result<std::sync::Arc<resolve::ImageFile>, String>> {
            let name = path.strip_suffix(".png")?;
            let (size, n) = name.split_once('-')?;
            let (w, h) = size.split_once('x')?;
            let (w, h): (u32, u32) = (w.parse().ok()?, h.parse().ok()?);
            Some(Ok(std::sync::Arc::new(resolve::ImageFile {
                id: resolve::ImageId::of(path.as_bytes()),
                bytes: n.as_bytes().to_vec(),
                width: w,
                height: h,
                // Zeroed pages the system hands out only once written.
                pixels: std::sync::Arc::new(vec![0; 4 * w as usize * h as usize]),
                pixels_width: w,
                pixels_height: h,
            })))
        }
    }

    /// Every image a drawing paints shares the GPU's one atlas. Images that
    /// don't fit together are the design's problem, at the `src` of the first
    /// that doesn't — never a hole in the canvas. One file shown many times
    /// takes its room once.
    #[test]
    fn images_that_do_not_fit_the_atlas_are_a_problem_where_written() {
        let design = |files: &[&str]| {
            let images: String = files
                .iter()
                .map(|f| format!("\n  <Image src=\"{f}\" width={{40}} height={{40}} />"))
                .collect();
            Sized(
                [(
                    "a.design".to_string(),
                    format!(r#"<Frame layout="row">{images}</Frame>"#),
                )]
                .into(),
            )
        };
        let mut r = Renderer::new();
        let four = design(&[
            "4096x4096-a.png",
            "4096x4096-b.png",
            "4096x4096-c.png",
            "4096x4096-d.png",
        ]);
        let scene = resolve::resolve(&four, "a.design").unwrap();
        assert!(r.drawing(&scene, Theme::Light, None).is_ok());
        // The same four again, and again: still four images.
        let again = design(&[
            "4096x4096-a.png",
            "4096x4096-b.png",
            "4096x4096-a.png",
            "4096x4096-c.png",
            "4096x4096-d.png",
            "4096x4096-b.png",
        ]);
        let scene = resolve::resolve(&again, "a.design").unwrap();
        assert!(r.drawing(&scene, Theme::Light, None).is_ok());
        let six = design(&[
            "4096x4096-a.png",
            "4096x4096-b.png",
            "4096x4096-c.png",
            "4096x4096-d.png",
            "4096x4096-e.png",
            "4096x4096-f.png",
        ]);
        let scene = resolve::resolve(&six, "a.design").unwrap();
        let Err(e) = r.drawing(&scene, Theme::Light, None) else {
            panic!("six 4096-pixel squares fit")
        };
        let at = e.at.unwrap();
        assert_eq!(
            (at.file.as_str(), at.pos.line, at.pos.col),
            ("a.design", 6, 10)
        );
        assert_eq!(
            e.message,
            "this doesn't fit: the images a drawing paints — its image files, at most 4096 \
             pixels a side, and its shadows — share the GPU's one 8192×8192 atlas, and the 4 \
             before this 4096×4096 fill it"
        );
    }

    /// A drawing made after another that paints the same images — files and
    /// shadows alike, in the same order — paints the last one's, under the
    /// same blobs, so the GPU keeps them in its atlas: an edit to text or
    /// layout uploads no image again. Any other change of images, and any
    /// drawing made after none, paints every image under a blob of its own.
    #[test]
    fn a_drawing_with_the_last_ones_images_paints_them_under_the_same_blobs() {
        let design = |text: &str, files: &[&str], blur: u32| {
            let images: String = files
                .iter()
                .map(|f| format!("\n  <Image src=\"{f}\" width={{40}} height={{40}} />"))
                .collect();
            let source = format!(
                r##"<Frame layout="row"><Text>{text}</Text>{images}
  <Rect width={{40}} height={{40}} shadow={{{{ blur: {blur}, color: "#000000" }}}} />
</Frame>"##
            );
            resolve::resolve(
                &Sized([("a.design".to_string(), source)].into()),
                "a.design",
            )
            .unwrap()
        };
        let ids = |d: &Drawing| d.images.iter().map(|i| i.data.id()).collect::<Vec<_>>();
        let painted = |d: &Drawing| {
            d.list
                .chunks()
                .flat_map(|(_, c)| c.commands.iter().map(|c| &c.draw))
                .filter_map(|c| match c {
                    Draw::Fill(_, Paint::Image(i, _)) => Some(i.data.id()),
                    _ => None,
                })
                .collect::<std::collections::BTreeSet<_>>()
        };
        let mut r = Renderer::new();
        let files = ["300x200-a.png", "100x100-b.png", "300x200-a.png"];
        let first = r
            .drawing(&design("One", &files, 6), Theme::Light, None)
            .unwrap();
        // Two files, one shown twice, and the shadow's images.
        assert!(first.images.len() > 2);
        assert_eq!(painted(&first), ids(&first).into_iter().collect());

        // New text and a new layout: the very same images.
        let edited = r
            .drawing(&design("Two words", &files, 6), Theme::Light, Some(&first))
            .unwrap();
        assert_eq!(ids(&edited), ids(&first));
        assert_eq!(painted(&edited), painted(&first));

        // A drawing made after none, and drawings whose images differ — a
        // file, their order, a shadow — share nothing with the last.
        let fresh = r
            .drawing(&design("One", &files, 6), Theme::Light, None)
            .unwrap();
        let other = ["300x200-a.png", "100x100-c.png"];
        let swapped = ["100x100-b.png", "300x200-a.png"];
        for d in [
            fresh,
            r.drawing(&design("One", &other, 6), Theme::Light, Some(&first))
                .unwrap(),
            r.drawing(&design("One", &swapped, 6), Theme::Light, Some(&first))
                .unwrap(),
            r.drawing(&design("One", &files, 7), Theme::Light, Some(&first))
                .unwrap(),
        ] {
            let shared: Vec<_> = ids(&d)
                .into_iter()
                .filter(|i| ids(&first).contains(i))
                .collect();
            assert!(shared.is_empty(), "{shared:?}");
            assert_eq!(painted(&d), ids(&d).into_iter().collect());
        }
    }

    #[test]
    fn a_root_that_fills_on_its_own_hugs() {
        let mut r = Renderer::new();
        for root in [
            r##"<Frame width="fill" height="fill" fill="#ff0000"><Rect width={100} height={50} /></Frame>"##,
            r##"<Frame width="fill" layout="row" fill="#ff0000"><Rect width={100} height={50} /></Frame>"##,
        ] {
            let d = r.drawing(&scene(root), Theme::Light, None).unwrap();
            assert_eq!((d.area().width, d.area().height), (100.0, 50.0), "{root}");
        }
    }

    #[test]
    fn a_drawing_is_laid_out_as_the_layout_lays_it_out() {
        let mut r = Renderer::new();
        let s = scene(r#"<Frame layout="row" padding={4}><Rect width={10} height={10} /></Frame>"#);
        let d = r.drawing(&s, Theme::Light, None).unwrap();
        assert_eq!(d.frames(), r.layout(&s));
        assert_eq!(d.list.chunks().count(), 2);
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
            "font",
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
                        // Laid out, made into a display list and its first
                        // tile painted: a drawing a million pixels a side is
                        // drawable, and painting all of it is not the point.
                        if let Ok(d) = r.drawing(&s, Theme::Light, None) {
                            let _ = crate::tiles::tile(&d.list, 1.0, 0, 0);
                        }
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
        fn list(&self, path: &str) -> Option<Vec<String>> {
            let mut names: Vec<String> = std::fs::read_dir(self.0.join(path))
                .ok()?
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            Some(names)
        }
    }

    fn repo() -> Repo {
        Repo(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".."))
    }

    /// Each example design, painted natively in each theme, matches its
    /// reference PNG in `design/testdata/` — `<name>.png` in the light theme,
    /// `<name>-dark.png` in the dark one — within the tolerance [`matches`]
    /// applies. After a deliberate change to
    /// how designs look, `DEVPAGE_UPDATE_REFERENCES=1 cargo test` rewrites
    /// them — and the diff shows the new drawings for review.
    #[test]
    fn the_examples_paint_as_their_references() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata");
        let update = std::env::var_os("DEVPAGE_UPDATE_REFERENCES").is_some();
        let mut renderer = Renderer::new();
        for (theme, suffix) in [(Theme::Light, ""), (Theme::Dark, "-dark")] {
            for name in ["button", "checkout", "confirmation", "details"] {
                let path = format!("docs/designs/checkout/{name}.design");
                let scene = crate::resolve(&repo(), &path).unwrap_or_else(|p| panic!("{p:?}"));
                assert!(scene.themed, "{name}");
                let png = crate::png(&mut renderer, &scene, theme).unwrap();
                let name = format!("{name}{suffix}");
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
    }

    /// The examples, painted tile by tile and streamed band by band, are the
    /// references the whole drawing painted in one went into, to within 2 of
    /// 255 in any channel; and a tile is that part of one paint of the whole.
    #[test]
    fn tiles_paint_the_examples_as_the_whole_drawing() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata");
        let mut renderer = Renderer::new();
        for (theme, suffix) in [(Theme::Light, ""), (Theme::Dark, "-dark")] {
            for name in ["button", "checkout", "confirmation", "details"] {
                let path = format!("docs/designs/checkout/{name}.design");
                let scene = crate::resolve(&repo(), &path).unwrap();
                let tiled = crate::png(&mut renderer, &scene, theme).unwrap();
                let whole = std::fs::read(dir.join(format!("{name}{suffix}.png"))).unwrap();
                let (a, w, h) = pixels(&whole).unwrap();
                let (b, tw, th) = pixels(&tiled).unwrap();
                assert_eq!((w, h), (tw, th), "{name}");
                let d = compare(&a, &b, w as usize).unwrap();
                assert!(d.within_tolerance(), "{name} {theme:?}: {d:?}");
                assert!(d.max <= 2, "{name} {theme:?}: {d:?}");
                // One paint of the whole drawing, and its first tile.
                let drawing = renderer.drawing(&scene, theme, None).unwrap();
                let one = paint_area(&drawing.list, frame_rect(drawing.area()), 1.0);
                let mut tile = crate::tiles::tile(&drawing.list, 1.0, 0, 0);
                let mut whole = one.data_as_u8_slice().to_vec();
                crate::tiles::unpremultiply(&mut whole);
                let side = crate::tiles::TILE as usize;
                for y in 0..side.min(h as usize) {
                    let n = side.min(w as usize) * 4;
                    let row = &whole[y * w as usize * 4..][..n];
                    let tr = &mut tile[y * side * 4..][..n];
                    for (p, q) in row.iter().zip(tr.iter()) {
                        assert!(p.abs_diff(*q) <= 1, "{name} tile row {y}");
                    }
                }
            }
        }
    }

    /// A design 100,000 pixels tall is drawn: its PNG streams band by band.
    #[test]
    fn a_design_100000_pixels_tall_has_a_png() {
        let mut rows = String::new();
        for i in 0..1000 {
            rows.push_str(&format!(
                "<Frame width={{400}} height={{100}} fill=\"#{:06x}\"><Text size={{20}}>Row {i}</Text></Frame>\n",
                (i * 7919) % 0xffffff
            ));
        }
        let files: HashMap<String, String> = [(
            "a.design".to_string(),
            format!("<Frame layout=\"column\" fill=\"#ffffff\">{rows}</Frame>"),
        )]
        .into();
        let s = resolve::resolve(&files, "a.design").unwrap();
        let mut r = Renderer::new();
        let t = std::time::Instant::now();
        let drawing = r.drawing(&s, Theme::Light, None).unwrap();
        let list = &drawing.list;
        let built = t.elapsed();
        assert_eq!((list.area.width, list.area.height), (400.0, 100_000.0));
        let t = std::time::Instant::now();
        let mut png = Vec::new();
        crate::tiles::png(list, &mut png).unwrap();
        eprintln!(
            "built {built:?}, png {:?}, {} bytes",
            t.elapsed(),
            png.len()
        );
        let (px, w, h) = pixels(&png).unwrap();
        assert_eq!((w, h), (400, 100_000));
        // Row 999's fill at its middle.
        let c = 999 * 7919;
        let at = ((99_950 * 400 + 390) * 4) as usize;
        assert_eq!(&px[at..at + 3], &[(c >> 16) as u8, (c >> 8) as u8, c as u8]);
        // A tile at the bottom paints only what touches it.
        let t = std::time::Instant::now();
        let found = list
            .query(crate::tiles::tile_rect(list, 1.0, 0, 390), 1.0)
            .len();
        eprintln!("query {:?}, {found} commands", t.elapsed());
        assert!(found < 12, "{found}");
    }

    /// An edit to one row of a tall design touches only the tiles around it,
    /// and the drawing it leaves paints as a fresh one.
    #[test]
    fn an_edit_to_one_node_damages_only_the_tiles_around_it() {
        let design = |label: &str| {
            let mut rows = String::new();
            for i in 0..1000 {
                let text = if i == 500 {
                    label.to_string()
                } else {
                    format!("Row {i}")
                };
                rows.push_str(&format!("<Frame width={{400}} height={{100}} fill=\"#eeeeee\"><Text size={{20}}>{text}</Text></Frame>\n"));
            }
            let files: HashMap<String, String> = [(
                "a.design".to_string(),
                format!("<Frame layout=\"column\" fill=\"#ffffff\">{rows}</Frame>"),
            )]
            .into();
            resolve::resolve(&files, "a.design").unwrap()
        };
        let (old, new) = (design("Row 500"), design("Edited"));
        let mut r = Renderer::new();
        let mut live = r.drawing(&old, Theme::Light, None).unwrap();
        let t = std::time::Instant::now();
        let damage = live.update(&mut r, &old, &new).unwrap();
        eprintln!("update {:?}, damage {damage:?}", t.elapsed());
        assert!(!damage.all && !damage.rects.is_empty());
        for d in &damage.rects {
            assert!(d.y0 >= 50_000.0 - 21.0 && d.y1 <= 50_100.0 + 21.0, "{d:?}");
        }
        let fresh = r.drawing(&new, Theme::Light, None).unwrap();
        for row in [0, 195, 196, 390] {
            let a = crate::tiles::tile(&live.list, 1.0, 0, row);
            let b = crate::tiles::tile(&fresh.list, 1.0, 0, row);
            assert!(a == b, "tile row {row}");
        }
    }

    /// A click on the 100,000-pixel design is answered from the index: the
    /// topmost node with an interaction under the point, and nothing where a
    /// clipping frame hides it.
    #[test]
    fn a_click_is_answered_from_the_index() {
        let mut rows = String::new();
        for i in 0..1000 {
            rows.push_str(&format!(
                "<Frame width={{400}} height={{100}} onClick=\"navigate ./r{i}.design\"><Frame width={{50}} height={{50}} clip><Rect x={{40}} width={{100}} height={{20}} onClick=\"back\" /></Frame></Frame>\n"
            ));
        }
        let mut files: HashMap<String, String> = (0..1000)
            .map(|i| (format!("r{i}.design"), "<Frame />".to_string()))
            .collect();
        files.insert(
            "a.design".to_string(),
            format!("<Frame layout=\"column\">{rows}</Frame>"),
        );
        let s = resolve::resolve(&files, "a.design").unwrap();
        let mut r = Renderer::new();
        let d = r.drawing(&s, Theme::Light, None).unwrap();
        assert_eq!(d.action_at(200.0, 99_950.0), Some("navigate r999.design"));
        // The rectangle, shown inside its clip, and hidden past it.
        assert_eq!(d.action_at(45.0, 99_905.0), Some("back"));
        assert_eq!(d.action_at(80.0, 99_905.0), Some("navigate r999.design"));
        assert_eq!(d.action_at(500.0, 99_905.0), None);
    }

    /// An update paints as a fresh drawing of the new scene does: a row
    /// growing moves those after it, a clipping parent resizing keeps its
    /// children under its clip, and a row added grows the area along its
    /// bottom — damage there, not everywhere; a child past the top-left
    /// moves the area's corner, which damages every tile.
    #[test]
    fn an_update_paints_as_a_fresh_drawing() {
        let design = |tall: u32, clip: u32, rows: usize, above: bool| {
            let mut body = String::new();
            for i in 0..rows {
                let h = if i == 3 { tall } else { 40 };
                body.push_str(&format!(
                    "<Frame width={{200}} height={{{h}}} fill=\"#{:06x}\" layout=\"none\"><Frame width={{{clip}}} height={{30}} clip shadow={{{{ blur: 6, color: \"#00000080\" }}}}><Rect x={{20}} width={{100}} height={{30}} fill=\"#ff0000\" /></Frame></Frame>",
                    i * 40503 % 0xffffff
                ));
            }
            let extra = if above {
                "<Rect x={-30} y={-30} width={10} height={10} fill=\"#00ff00\" />"
            } else {
                ""
            };
            let files: HashMap<String, String> = [(
                "a.design".to_string(),
                format!("<Frame layout=\"none\" fill=\"#ffffff\"><Frame layout=\"column\">{body}</Frame>{extra}</Frame>"),
            )]
            .into();
            resolve::resolve(&files, "a.design").unwrap()
        };
        let mut r = Renderer::new();
        let paint = |d: &Drawing| {
            paint_area(&d.list, frame_rect(d.area()), 1.0)
                .data_as_u8_slice()
                .to_vec()
        };
        let steps = [
            design(40, 60, 8, false),
            design(90, 60, 8, false),
            design(90, 150, 8, false),
            design(90, 150, 9, false),
            design(90, 150, 9, true),
            design(40, 60, 6, false),
        ];
        let mut live = r.drawing(&steps[0], Theme::Light, None).unwrap();
        for (n, pair) in steps.windows(2).enumerate() {
            let damage = live.update(&mut r, &pair[0], &pair[1]).unwrap();
            let fresh = r.drawing(&pair[1], Theme::Light, None).unwrap();
            assert_eq!(live.area(), fresh.area(), "step {n}");
            assert_eq!(live.frames(), fresh.frames(), "step {n}");
            assert!(paint(&live) == paint(&fresh), "step {n}");
            match n {
                2 => {
                    assert!(!damage.all);
                    let bottom = live.area().y as f64 + live.area().height as f64;
                    assert!(
                        damage.rects.iter().any(|d| d.y1 >= bottom - 1.0),
                        "{damage:?}"
                    );
                }
                3 => assert!(damage.all, "{damage:?}"),
                _ => {}
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
}
