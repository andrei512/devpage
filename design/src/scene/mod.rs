//! What a resolved design is, and how one becomes the next. A [`Scene`] is
//! a tree of [`Node`]s — each with its own attributes, colours in both
//! [`Theme`]s, and an id that survives saves — with the image and font files
//! it shows, read through the [`Files`] port. An [`Edit`] names its nodes by
//! id; [`Edits`] are a list of them with the files they bring, and applying
//! them ([`apply`]) is all or nothing, undone on a refusal, at the cost of
//! the edits alone. [`Applied`] says what they did, for a drawing kept
//! beside the scene to follow.
//!
//! It is what every other part of the engine speaks: [`crate::resolve`]
//! makes scenes and their edits, [`crate::render`] draws them, and
//! [`crate::wire`] carries both to the browser.

use std::collections::HashMap;
use std::sync::Arc;

use peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};

use crate::Problem;
use crate::fonts::{FontFile, FontFiles, FontId};
use crate::parse::{self, Pos};
use crate::tree::{Children, NodeId, Order, OrderKey};

/// One color's value: straight-alpha RGBA.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    /// The red channel, 0 to 255.
    pub r: u8,
    /// The green channel, 0 to 255.
    pub g: u8,
    /// The blue channel, 0 to 255.
    pub b: u8,
    /// The alpha, 0 transparent to 255 opaque; the other channels are not
    /// multiplied by it.
    pub a: u8,
}

/// Which of a design's two themes it is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Theme {
    /// The light theme, the default: each color's `light` value.
    #[default]
    Light,
    /// The dark theme: each color's `dark` value.
    Dark,
}

impl Theme {
    /// `"light"` or `"dark"` as a theme, as a PNG's `?theme=` names one.
    pub fn named(name: &str) -> Option<Theme> {
        match name {
            "light" => Some(Theme::Light),
            "dark" => Some(Theme::Dark),
            _ => None,
        }
    }
}

/// A color in a scene: its value in each theme. A color written as `#…` has
/// the same value in both; `theme.name` has the themes' two.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    /// The value in the light theme.
    pub light: Rgba,
    /// The value in the dark theme.
    pub dark: Rgba,
}

impl Color {
    /// The same value in both themes.
    pub const fn fixed(c: Rgba) -> Color {
        Color { light: c, dark: c }
    }

    /// The value in `theme`.
    pub fn get(self, theme: Theme) -> Rgba {
        match theme {
            Theme::Light => self.light,
            Theme::Dark => self.dark,
        }
    }
}

/// How a node's width or height is set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Size {
    /// A fixed size, in pixels.
    Px(f32),
    /// As large as the node's content.
    Hug,
    /// The room its parent's auto layout gives it: the free space along the
    /// layout, the whole breadth across it. A root, with no parent, hugs.
    Fill,
}

/// How a frame places its children.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Layout {
    /// Each child where its `x` and `y` say.
    None,
    /// Side by side, left to right.
    Row,
    /// One under another, top to bottom.
    Column,
}

/// Where a frame's auto layout puts its children along one axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Align {
    /// At the start.
    Start,
    /// In the middle.
    Center,
    /// At the end.
    End,
    /// The first at the start, the last at the end, and the free space shared
    /// between them; `justify` only.
    SpaceBetween,
}

/// What a node is: the element that made it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// A box that holds children and may lay them out.
    Frame,
    /// Text, set in one style.
    Text,
    /// A rectangle.
    Rect,
    /// An ellipse filling the node's box.
    Ellipse,
    /// An SVG path.
    Vector,
    /// A PNG or JPEG file.
    Image,
}

/// How a box is filled: one color, or a CSS-style linear gradient.
#[derive(Debug, Clone, PartialEq)]
pub enum Fill {
    /// One color over the whole fill, with a value for each theme.
    Solid(Color),
    /// `angle` in degrees, CSS's: 0 points up, 90 right, 180 down. Each stop
    /// is a color at an offset from 0 to 1 along the gradient line.
    Linear {
        angle: f32,
        stops: Vec<(Color, f32)>,
    },
}

/// How each line of a text sits across its box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TextAlign {
    /// Against the left edge.
    Left,
    /// In the middle.
    Center,
    /// Against the right edge.
    Right,
}

/// A `Vector`'s path and the box of path coordinates it is drawn from.
#[derive(Debug, Clone, PartialEq)]
pub struct Vector {
    /// The path, as SVG path data.
    pub d: String,
    /// `[minX, minY, width, height]`.
    pub view_box: [f32; 4],
}

/// An image file's identity: the SHA-256 of its bytes. Two `Image`s showing
/// the same bytes — the same file used twice, or two copies of it — share one
/// [`ImageFile`], decoded once and sent once.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImageId(/** Digest identifying the image bytes. */ pub [u8; 32]);

impl ImageId {
    /// The identity of the file whose bytes are `bytes`.
    pub fn of(bytes: &[u8]) -> ImageId {
        use sha2::Digest;
        ImageId(sha2::Sha256::digest(bytes).into())
    }
}

impl std::fmt::Debug for ImageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for b in &self.0[..6] {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

/// The most pixels a side an image is drawn from. The GPU keeps every image a
/// drawing paints in one atlas of [`crate::atlas::SIDE`] pixels a side, so a
/// larger file is resampled to fit this, keeping its aspect, when it is
/// decoded — and both backends draw those same pixels.
pub const MAX_IMAGE_SIDE: u32 = 4096;

/// A decoded image file: its bytes, its own size, and the pixels it is drawn
/// from — the file's own, or resampled to at most [`MAX_IMAGE_SIDE`] a side.
#[derive(Debug)]
pub struct ImageFile {
    /// Its identity: the SHA-256 of its bytes.
    pub id: ImageId,
    /// The file's bytes, as read.
    pub bytes: Vec<u8>,
    /// The file's size in pixels: what an `Image` hugs by.
    pub width: u32,
    /// The file's height in pixels.
    pub height: u32,
    /// Straight-alpha RGBA, `pixels_width` × `pixels_height`.
    pub pixels: Arc<Vec<u8>>,
    /// The width of `pixels`, in pixels.
    pub pixels_width: u32,
    /// The height of `pixels`, in pixels.
    pub pixels_height: u32,
}

impl ImageFile {
    /// Decodes `bytes`, a PNG or JPEG file, or says why it can't.
    pub fn new(bytes: Vec<u8>) -> Result<ImageFile, String> {
        ImageFile::with_id(ImageId::of(&bytes), bytes)
    }

    /// Decodes `bytes`, already known to be the file `id`.
    pub fn with_id(id: ImageId, bytes: Vec<u8>) -> Result<ImageFile, String> {
        let decoded = decode(&bytes)?;
        let (width, height) = (decoded.width, decoded.height);
        let (pixels, pixels_width, pixels_height) =
            fit(decoded.data.data(), width, height, MAX_IMAGE_SIDE);
        Ok(ImageFile {
            id,
            bytes,
            width,
            height,
            pixels: Arc::new(pixels),
            pixels_width,
            pixels_height,
        })
    }

    /// The pixels as an image for one drawing to paint, under a blob of its
    /// own that shares the pixels: see `render::Renderer::drawing`.
    pub fn image_data(&self) -> ImageData {
        ImageData {
            data: Blob::new(self.pixels.clone()),
            format: ImageFormat::Rgba8,
            alpha_type: ImageAlphaType::Alpha,
            width: self.pixels_width,
            height: self.pixels_height,
        }
    }
}

/// `rgba`, `width` × `height`, resampled to at most `max` pixels a side with
/// its aspect kept, or as it is when it already fits. Each new pixel is the
/// area-weighted average of the pixels under it, in premultiplied alpha so a
/// transparent pixel lends no color.
pub fn fit(rgba: &[u8], width: u32, height: u32, max: u32) -> (Vec<u8>, u32, u32) {
    if width <= max && height <= max {
        return (rgba.to_vec(), width, height);
    }
    let scale = max as f64 / width.max(height) as f64;
    let w = ((width as f64 * scale).round() as u32).clamp(1, max);
    let h = ((height as f64 * scale).round() as u32).clamp(1, max);
    // For each output index along one axis, the input indices it covers and
    // how much of each.
    let weights = |from: u32, to: u32| -> Vec<Vec<(usize, f32)>> {
        let ratio = from as f64 / to as f64;
        (0..to)
            .map(|o| {
                let (a, b) = (o as f64 * ratio, (o + 1) as f64 * ratio);
                (a.floor() as usize..(b.ceil() as usize).min(from as usize))
                    .map(|i| {
                        let cover = (b.min(i as f64 + 1.0) - a.max(i as f64)) / ratio;
                        (i, cover as f32)
                    })
                    .filter(|&(_, c)| c > 0.0)
                    .collect()
            })
            .collect()
    };
    let (across, down) = (weights(width, w), weights(height, h));
    // Across each row first, premultiplied, then down each column.
    let mut rows = vec![0f32; 4 * w as usize * height as usize];
    for y in 0..height as usize {
        let line = &rgba[4 * y * width as usize..][..4 * width as usize];
        for (x, ws) in across.iter().enumerate() {
            let mut sum = [0f32; 4];
            for &(i, c) in ws {
                let p = &line[4 * i..][..4];
                let a = p[3] as f32 * c;
                sum[0] += p[0] as f32 * a;
                sum[1] += p[1] as f32 * a;
                sum[2] += p[2] as f32 * a;
                sum[3] += a;
            }
            rows[4 * (y * w as usize + x)..][..4].copy_from_slice(&sum);
        }
    }
    let mut out = vec![0u8; 4 * w as usize * h as usize];
    for (y, ws) in down.iter().enumerate() {
        for x in 0..w as usize {
            let mut sum = [0f32; 4];
            for &(i, c) in ws {
                let p = &rows[4 * (i * w as usize + x)..][..4];
                for k in 0..4 {
                    sum[k] += p[k] * c;
                }
            }
            let px = &mut out[4 * (y * w as usize + x)..][..4];
            if sum[3] > 0.0 {
                for k in 0..3 {
                    px[k] = (sum[k] / sum[3]).round().clamp(0.0, 255.0) as u8;
                }
                px[3] = sum[3].round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    (out, w, h)
}

/// Where an attribute was written, for a problem found only once the design
/// is laid out and painted. It is never part of what a node draws: two nodes
/// that differ only in it are equal, and it doesn't travel on the wire.
#[derive(Debug, Clone)]
pub struct Origin {
    /// The design file it was written in, by its path from the top of the
    /// served folder.
    pub file: String,
    /// Where in that file it was written.
    pub pos: Pos,
}

impl Default for Origin {
    fn default() -> Origin {
        Origin {
            file: String::new(),
            pos: Pos { line: 1, col: 1 },
        }
    }
}

impl PartialEq for Origin {
    fn eq(&self, _: &Origin) -> bool {
        true
    }
}

/// An `Image`'s file: its address in the served folder and which
/// [`ImageFile`] in the scene it shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    /// The image file's path from the top of the served folder.
    pub src: String,
    /// The [`ImageFile`] it shows, by its id in the scene's `images`.
    pub id: ImageId,
    /// Where `src` was written.
    pub at: Origin,
}

/// A PNG or JPEG file's pixels, as straight-alpha RGBA, or why it has none.
pub fn decode(data: &[u8]) -> Result<ImageData, String> {
    let (pixels, width, height) = if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        let broken = |e: &dyn std::fmt::Display| format!("is a PNG that doesn't decode: {e}");
        let mut decoder = png::Decoder::new(std::io::Cursor::new(data));
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        let mut reader = decoder.read_info().map_err(|e| broken(&e))?;
        let size = reader
            .output_buffer_size()
            .ok_or_else(|| broken(&"too big to hold"))?;
        let mut buf = vec![0; size];
        let info = reader.next_frame(&mut buf).map_err(|e| broken(&e))?;
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
            // Expanded to RGB by `normalize_to_color8`.
            png::ColorType::Indexed => return Err(broken(&"an unexpanded palette")),
        };
        (rgba, info.width, info.height)
    } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
        use zune_jpeg::JpegDecoder;
        use zune_jpeg::zune_core::colorspace::ColorSpace;
        use zune_jpeg::zune_core::options::DecoderOptions;
        let broken = |e: &dyn std::fmt::Display| format!("is a JPEG that doesn't decode: {e}");
        let options = DecoderOptions::default()
            .set_strict_mode(true)
            .jpeg_set_out_colorspace(ColorSpace::RGBA);
        let mut decoder = JpegDecoder::new_with_options(std::io::Cursor::new(data), options);
        let pixels = decoder.decode().map_err(|e| broken(&e))?;
        let info = decoder.info().ok_or_else(|| broken(&"no header"))?;
        (pixels, info.width as u32, info.height as u32)
    } else {
        return Err("is not a PNG or JPEG file".into());
    };
    if width == 0 || height == 0 || pixels.len() != width as usize * height as usize * 4 {
        return Err(format!(
            "is an image of {width}×{height} pixels that doesn't decode whole"
        ));
    }
    Ok(ImageData {
        data: Blob::new(Arc::new(pixels)),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width,
        height,
    })
}

/// A box's drop shadow, as CSS's `box-shadow` draws one: an offset, a blur and a
/// spread, in pixels, and a color.
#[derive(Debug, Clone, PartialEq)]
pub struct Shadow {
    /// How far right of the box the shadow is, in pixels.
    pub x: f32,
    /// How far down from the box the shadow is, in pixels.
    pub y: f32,
    /// How far the shadow blurs, in pixels: twice the Gaussian's standard
    /// deviation.
    pub blur: f32,
    /// How far the shadow grows past the box on every side before it blurs,
    /// in pixels; a negative spread shrinks it.
    pub spread: f32,
    /// The shadow's color.
    pub color: Color,
    /// Where `shadow` was written.
    pub at: Origin,
}

/// A `Text`'s text and how it is set.
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    /// The font file the text is set in, the face its weight picked; `None`
    /// for Inter.
    pub font: Option<FontId>,
    /// What the text says: the element's text, each `{prop}` in it filled
    /// in — a number as Rust prints it.
    pub content: String,
    /// The font size, in pixels.
    pub size: f32,
    /// The font weight: 100 to 900, in steps of 100.
    pub weight: u16,
    /// The text's color.
    pub color: Color,
    /// The height of a line, as a multiple of `size`.
    pub line_height: f32,
    /// How each line sits across the text's box.
    pub align: TextAlign,
}

/// One node of a scene.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// Where the node came from — see [`NodeId`] — which no edit around it
    /// changes.
    pub id: NodeId,
    /// What the node is.
    pub kind: Kind,
    /// The node's name: its `name` attribute, or else its element's, as
    /// problems call it.
    pub name: String,
    /// How far right of its parent's top-left the node sits, in pixels; only a
    /// parent with no auto layout places it by this.
    pub x: f32,
    /// How far down from its parent's top-left the node sits, in pixels; only a
    /// parent with no auto layout places it by this.
    pub y: f32,
    /// How the node's width is set.
    pub width: Size,
    /// How the node's height is set.
    pub height: Size,
    /// How opaque the node and everything in it is, from 0 to 1.
    pub opacity: f32,
    /// What a click on the node does: `navigate <path>`, `back`, `overlay
    /// <path>` or `close`, each path from the top of the served folder. `None`
    /// when a click does nothing.
    pub on_click: Option<String>,
    /// What the node's box is filled with; `None` for nothing.
    pub fill: Option<Fill>,
    /// The color of the node's outline; `None` for no outline.
    pub stroke: Option<Color>,
    /// How wide the outline is, in pixels.
    pub stroke_width: f32,
    /// Each corner's radius, in pixels: top-left, top-right, bottom-right,
    /// bottom-left.
    pub radius: [f32; 4],
    /// The node's drop shadow; `None` for none.
    pub shadow: Option<Shadow>,
    /// Whether a frame's children are cut off at its box, rounded corners
    /// and all.
    pub clip: bool,
    /// How a frame places its children.
    pub layout: Layout,
    /// The space between a frame's children in its auto layout, in pixels.
    pub gap: f32,
    /// The space inside a frame's edges, in pixels: top, right, bottom, left.
    pub padding: [f32; 4],
    /// Where a frame's auto layout puts its children across its direction.
    pub align: Align,
    /// Where a frame's auto layout puts its children along its direction.
    pub justify: Align,
    /// A `Text`'s text and how it is set; `None` for any other kind.
    pub text: Option<TextStyle>,
    /// A `Vector`'s path; `None` for any other kind.
    pub vector: Option<Vector>,
    /// An `Image`'s file; `None` for any other kind.
    pub image: Option<Image>,
    /// The node's children, in order.
    pub children: Children<Node>,
}

impl Node {
    /// Whether two nodes' own attributes are the same, their children aside.
    pub fn same_own(&self, b: &Node) -> bool {
        let a = self;
        a.kind == b.kind
            && a.name == b.name
            && a.x == b.x
            && a.y == b.y
            && a.width == b.width
            && a.height == b.height
            && a.opacity == b.opacity
            && a.on_click == b.on_click
            && a.fill == b.fill
            && a.stroke == b.stroke
            && a.stroke_width == b.stroke_width
            && a.radius == b.radius
            && a.shadow == b.shadow
            && a.clip == b.clip
            && a.layout == b.layout
            && a.gap == b.gap
            && a.padding == b.padding
            && a.align == b.align
            && a.justify == b.justify
            && a.text == b.text
            && a.vector == b.vector
            && a.image == b.image
    }

    /// The node's own attributes, without its children.
    pub fn own(&self) -> Node {
        Node {
            id: self.id.clone(),
            kind: self.kind,
            name: self.name.clone(),
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            opacity: self.opacity,
            on_click: self.on_click.clone(),
            fill: self.fill.clone(),
            stroke: self.stroke,
            stroke_width: self.stroke_width,
            radius: self.radius,
            shadow: self.shadow.clone(),
            clip: self.clip,
            layout: self.layout,
            gap: self.gap,
            padding: self.padding,
            align: self.align,
            justify: self.justify,
            text: self.text.clone(),
            vector: self.vector.clone(),
            image: self.image.clone(),
            children: Children::new(),
        }
    }

    /// A node of `kind` with every attribute at its default.
    pub(crate) fn new(kind: Kind, name: &str) -> Node {
        Node {
            id: NodeId::default(),
            kind,
            name: name.to_string(),
            x: 0.0,
            y: 0.0,
            width: Size::Hug,
            height: Size::Hug,
            opacity: 1.0,
            on_click: None,
            fill: None,
            stroke: None,
            stroke_width: 1.0,
            radius: [0.0; 4],
            shadow: None,
            clip: false,
            layout: Layout::None,
            gap: 0.0,
            padding: [0.0; 4],
            align: Align::Start,
            justify: Align::Start,
            text: None,
            vector: None,
            image: None,
            children: Children::new(),
        }
    }
}

/// Reads a design's files by their path from the top of the served folder.
pub trait Files {
    /// A file's bytes: a design's text, an `Image`'s file or a font file.
    fn bytes(&self, path: &str) -> Option<Vec<u8>>;
    /// The names of the files directly in the folder at `path`, sorted;
    /// `None` when there is no such folder. A font folder is read this way.
    fn list(&self, path: &str) -> Option<Vec<String>>;
    /// A design file's text.
    fn read(&self, path: &str) -> Option<String> {
        String::from_utf8(self.bytes(path)?).ok()
    }
    /// The design file at `path`, whose text is `source`, parsed. A store
    /// that keeps files between resolves matches it against the file's last
    /// parse ([`crate::ids::matched`]), so its elements keep their ids — and
    /// the nodes they make theirs — across saves.
    fn parse(&self, path: &str, source: &str) -> Result<Arc<parse::File>, Problem> {
        let _ = path;
        parse::parse(source).map(Arc::new)
    }
    /// The image file at `path`, decoded, or why it doesn't decode; `None`
    /// when there is no such file. A store that keeps files between resolves
    /// keeps their decoded images too, so a save doesn't decode them again.
    fn image(&self, path: &str) -> Option<Result<Arc<ImageFile>, String>> {
        self.bytes(path)
            .map(|bytes| ImageFile::new(bytes).map(Arc::new))
    }
    /// The font file at `path`, checked, or why it isn't one; `None` when
    /// there is no such file. A store that keeps files between resolves
    /// keeps them checked too.
    fn font(&self, path: &str) -> Option<Result<Arc<FontFile>, String>> {
        self.bytes(path)
            .map(|bytes| FontFile::new(bytes).map(Arc::new))
    }
}

/// The names of the files directly in `folder`, from every path a store
/// holds — `None` when it holds none there.
pub fn listed<'a>(paths: impl Iterator<Item = &'a String>, folder: &str) -> Option<Vec<String>> {
    let prefix = format!("{folder}/");
    let mut names: Vec<String> = paths
        .filter_map(|p| p.strip_prefix(&prefix))
        .filter(|rest| !rest.contains('/'))
        .map(str::to_string)
        .collect();
    names.sort();
    (!names.is_empty()).then_some(names)
}

impl Files for HashMap<String, String> {
    fn bytes(&self, path: &str) -> Option<Vec<u8>> {
        self.get(path).map(|s| s.clone().into_bytes())
    }
    fn list(&self, path: &str) -> Option<Vec<String>> {
        listed(self.keys(), path)
    }
}

/// A resolved design: its tree, each distinct image file its `Image`s show and
/// font file its texts are set in, and whether it is **themed** — whether any
/// `theme.` reference reached its nodes.
#[derive(Debug, Clone)]
pub struct Scene {
    /// The tree's root node: the root design's `Frame`.
    pub root: Node,
    /// Each distinct image file the scene's `Image`s show, by id.
    pub images: ImageFiles,
    /// Each font file the scene's texts are set in, by id.
    pub fonts: FontFiles,
    /// Whether any `theme.` reference reached the scene's nodes, so its two
    /// themes can differ.
    pub themed: bool,
    /// Where each node is, by its id: what a patch finds its nodes by.
    pub ids: Ids,
}

/// Two scenes are the same when they hold the same nodes, image and font
/// files and themes, however their children were placed.
impl PartialEq for Scene {
    fn eq(&self, other: &Scene) -> bool {
        self.root == other.root
            && self.images == other.images
            && self.fonts == other.fonts
            && self.themed == other.themed
    }
}

impl Scene {
    /// A scene of `root` and its files, its ids indexed.
    pub fn new(root: Node, images: ImageFiles, fonts: FontFiles, themed: bool) -> Scene {
        let ids = Ids::of(&root);
        Scene {
            root,
            images,
            fonts,
            themed,
            ids,
        }
    }

    /// The node with id `id`, if the scene has it.
    pub fn node(&self, id: &NodeId) -> Option<&Node> {
        self.at(self.ids.get(id)?)
    }

    /// The node at `order`, if any.
    pub fn at(&self, order: &[OrderKey]) -> Option<&Node> {
        let mut at = &self.root;
        for k in order {
            at = at.children.get(k)?;
        }
        Some(at)
    }

    /// The node at `order`, to change, if any.
    pub fn at_mut(&mut self, order: &[OrderKey]) -> Option<&mut Node> {
        let mut at = &mut self.root;
        for k in order {
            at = at.children.get_mut(k)?;
        }
        Some(at)
    }
}

/// Where each node of a scene is, by its id: what a patch finds its nodes
/// by.
#[derive(Clone, Debug, Default)]
pub struct Ids(/** Scene order assigned to each node. */ HashMap<NodeId, Order>);

impl Ids {
    /// Every node of the tree under `root`.
    pub fn of(root: &Node) -> Ids {
        let mut ids = Ids::default();
        ids.add(root, Vec::new());
        ids
    }

    /// `n` and the nodes under it, `n` at `order`.
    pub fn add(&mut self, n: &Node, order: Order) {
        for (k, c) in n.children.entries() {
            let mut o = order.clone();
            o.push(k.clone());
            self.add(c, o);
        }
        self.0.insert(n.id.clone(), order);
    }

    /// Forgets `n` and the nodes under it.
    pub fn remove(&mut self, n: &Node) {
        self.0.remove(&n.id);
        for c in &n.children {
            self.remove(c);
        }
    }

    /// Where the node `id` is; `None` when the scene has no such node.
    pub fn get(&self, id: &NodeId) -> Option<&Order> {
        self.0.get(id)
    }

    /// Whether the scene has a node `id`.
    pub fn contains(&self, id: &NodeId) -> bool {
        self.0.contains_key(id)
    }

    /// How many nodes the scene has.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the scene has no nodes.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A scene's image files by id, each decoded once however many `Image`s
/// show it.
pub type ImageFiles = std::collections::BTreeMap<ImageId, Arc<ImageFile>>;

/// Image files compare by their identity, which is their bytes.
impl PartialEq for ImageFile {
    fn eq(&self, other: &ImageFile) -> bool {
        self.id == other.id
    }
}

mod edit;
#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use edit::Bare;
pub use edit::{Applied, Edit, Edits, Holder, Undo, Uses, applied, apply, subtree};
