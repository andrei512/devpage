//! Syntax tree → scene: imports and props substituted, every attribute
//! checked, typed and defaulted.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;
use std::sync::Arc;

use peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};

use crate::Problem;
use crate::fonts::{self, Family, FontFile, FontFiles, FontId};
use crate::parse::{self, Child, Element, Pos, TextPart, Value};
use crate::tree::{Children, NodeId, Order, OrderKey};

/// One color's value: straight-alpha RGBA.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// Which of a design's two themes it is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Theme {
    #[default]
    Light,
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
    pub light: Rgba,
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Size {
    Px(f32),
    Hug,
    Fill,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Layout {
    None,
    Row,
    Column,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Align {
    Start,
    Center,
    End,
    SpaceBetween,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Frame,
    Text,
    Rect,
    Ellipse,
    Vector,
    Image,
}

/// How a box is filled: one color, or a CSS-style linear gradient.
#[derive(Debug, Clone, PartialEq)]
pub enum Fill {
    Solid(Color),
    /// `angle` in degrees, CSS's: 0 points up, 90 right, 180 down. Each stop
    /// is a color at an offset from 0 to 1 along the gradient line.
    Linear {
        angle: f32,
        stops: Vec<(Color, f32)>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

/// A `Vector`'s path and the box of path coordinates it is drawn from.
#[derive(Debug, Clone, PartialEq)]
pub struct Vector {
    pub d: String,
    /// `[minX, minY, width, height]`.
    pub view_box: [f32; 4],
}

/// An image file's identity: the SHA-256 of its bytes. Two `Image`s showing
/// the same bytes — the same file used twice, or two copies of it — share one
/// [`ImageFile`], decoded once and sent once.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImageId(pub [u8; 32]);

impl ImageId {
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
    pub id: ImageId,
    pub bytes: Vec<u8>,
    /// The file's size in pixels: what an `Image` hugs by.
    pub width: u32,
    pub height: u32,
    /// Straight-alpha RGBA, `pixels_width` × `pixels_height`.
    pub pixels: Arc<Vec<u8>>,
    pub pixels_width: u32,
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
    pub file: String,
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
    pub src: String,
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

#[derive(Debug, Clone, PartialEq)]
pub struct Shadow {
    pub x: f32,
    pub y: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Color,
    /// Where `shadow` was written.
    pub at: Origin,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    /// The font file the text is set in, the face its weight picked; `None`
    /// for Inter.
    pub font: Option<FontId>,
    pub content: String,
    pub size: f32,
    pub weight: u16,
    pub color: Color,
    pub line_height: f32,
    pub align: TextAlign,
}

/// One node of a scene.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// Where the node came from — see [`NodeId`] — which no edit around it
    /// changes.
    pub id: NodeId,
    pub kind: Kind,
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub width: Size,
    pub height: Size,
    pub opacity: f32,
    pub on_click: Option<String>,
    pub fill: Option<Fill>,
    pub stroke: Option<Color>,
    pub stroke_width: f32,
    pub radius: [f32; 4],
    pub shadow: Option<Shadow>,
    pub clip: bool,
    pub layout: Layout,
    pub gap: f32,
    pub padding: [f32; 4],
    pub align: Align,
    pub justify: Align,
    pub text: Option<TextStyle>,
    pub vector: Option<Vector>,
    pub image: Option<Image>,
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

    fn new(kind: Kind, name: &str) -> Node {
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

/// A resolved design: its tree, each distinct image file its `Image`s show
/// and font file its texts are set in, the files it was made from, and
/// whether it is **themed** — whether any `theme.` reference reached its
/// nodes.
#[derive(Debug, Clone)]
pub struct Scene {
    pub root: Node,
    pub images: ImageFiles,
    pub fonts: FontFiles,
    pub files: Vec<String>,
    pub themed: bool,
    /// Where each node is, by its id: what a patch finds its nodes by.
    pub ids: Ids,
}

/// Two scenes are the same when they hold the same nodes, files and
/// themes, however their children were placed.
impl PartialEq for Scene {
    fn eq(&self, other: &Scene) -> bool {
        self.root == other.root
            && self.images == other.images
            && self.fonts == other.fonts
            && self.files == other.files
            && self.themed == other.themed
    }
}

impl Scene {
    /// A scene of `root` and its files, its ids indexed.
    pub fn new(
        root: Node,
        images: ImageFiles,
        fonts: FontFiles,
        files: Vec<String>,
        themed: bool,
    ) -> Scene {
        let ids = Ids::of(&root);
        Scene {
            root,
            images,
            fonts,
            files,
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
pub struct Ids(HashMap<NodeId, Order>);

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

    pub fn get(&self, id: &NodeId) -> Option<&Order> {
        self.0.get(id)
    }

    pub fn contains(&self, id: &NodeId) -> bool {
        self.0.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

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

/// Resolves the design at `path`, reporting every problem found.
pub fn resolve(files: &dyn Files, path: &str) -> Result<Scene, Vec<Problem>> {
    resolved(files, path).map(|(scene, _)| scene)
}

/// The design at `path` resolved, with the record of where each node came
/// from and what it read — what resolving it again starts from.
fn resolved(files: &dyn Files, path: &str) -> Result<(Scene, Record), Vec<Problem>> {
    let mut r = Resolver::new(files);
    let root = r.design(path, &[], None, "", None);
    if r.problems.is_empty() {
        let scene = Scene::new(
            root.expect("no problems means a root"),
            r.images,
            r.fonts,
            r.read,
            r.themed,
        );
        Ok((scene, r.record))
    } else {
        Err(r.problems)
    }
}

impl<'a> Resolver<'a> {
    fn new(files: &'a dyn Files) -> Resolver<'a> {
        Resolver {
            files,
            problems: Vec::new(),
            read: Vec::new(),
            stack: Vec::new(),
            missing: Vec::new(),
            depth: 0,
            images: ImageFiles::new(),
            by_path: HashMap::new(),
            fonts: FontFiles::new(),
            families: HashMap::new(),
            themes: HashMap::new(),
            themed: false,
            instance: Vec::new(),
            record: Record::default(),
            current: None,
            own_only: false,
        }
    }
}

struct Resolver<'a> {
    files: &'a dyn Files,
    problems: Vec<Problem>,
    read: Vec<String>,
    stack: Vec<String>,
    /// Imports already reported as `no such file`, so a use of one isn't
    /// reported a second time.
    missing: Vec<String>,
    /// How many elements enclose the current one, across instances.
    depth: usize,
    /// Every image file the design shows, by id.
    images: ImageFiles,
    /// Each image file read so far, by its path.
    by_path: HashMap<String, Result<ImageId, String>>,
    /// Every font file the design's texts are set in, by id.
    fonts: FontFiles,
    /// Each font a `themes.yml` named and a text used so far, file or
    /// folder, by its path, and whether its problem was reported yet: once
    /// per resolve, however many texts use it.
    families: HashMap<String, (Found, bool)>,
    /// The themes each folder's designs use, merged from every `themes.yml`
    /// from that folder up, by folder: read once per resolve.
    themes: HashMap<String, Rc<Themes>>,
    /// Whether a `theme.` reference reached a node.
    themed: bool,
    /// The element ids of the instances enclosing the element being
    /// resolved, outermost first: the start of every id made here.
    instance: Vec<u32>,
    /// Where each node made came from and what it read.
    record: Record,
    /// The node whose attributes are being resolved: what each file read
    /// now is read by.
    current: Option<NodeId>,
    /// Whether to resolve an element's own attributes only, leaving its
    /// child elements alone: resolving one node again.
    own_only: bool,
}

/// Where a resolved design's nodes came from and what they read: what
/// [`Resolution`] resolves again from when a file changes.
#[derive(Debug, Clone, Default)]
struct Record {
    /// Each instance, by the element ids leading to it — the root design's
    /// is empty.
    instances: BTreeMap<Vec<u32>, Instance>,
    /// The instances of each design file.
    by_file: HashMap<String, BTreeSet<Vec<u32>>>,
    /// The nodes and instances that read each file — or folder, by its path
    /// and a `/` — by id: a change to it resolves them again.
    readers: HashMap<String, BTreeSet<Vec<u32>>>,
    /// What each node or instance reads, so it can be forgotten.
    reads: BTreeMap<Vec<u32>, BTreeSet<String>>,
    /// The nodes a `theme.` reference reached.
    themed: BTreeSet<NodeId>,
    /// The parse each design file was resolved from.
    parses: HashMap<String, Arc<parse::File>>,
}

impl Record {
    /// `id`, a node's or an instance's, reads `key`.
    fn read_by(&mut self, key: &str, id: &[u32]) {
        self.readers
            .entry(key.to_string())
            .or_default()
            .insert(id.to_vec());
        self.reads
            .entry(id.to_vec())
            .or_default()
            .insert(key.to_string());
    }

    /// `id` reads nothing any more.
    fn forget(&mut self, id: &[u32]) {
        if let Some(keys) = self.reads.remove(id) {
            for key in keys {
                if let Some(readers) = self.readers.get_mut(&key) {
                    readers.remove(id);
                    if readers.is_empty() {
                        self.readers.remove(&key);
                    }
                }
            }
        }
    }
}

/// One use of a design file: the props its elements are resolved in, its
/// imports, the layout of the frame it sits in, the caller's attributes laid
/// over its root, and its root node's id.
#[derive(Debug, Clone)]
struct Instance {
    file: String,
    props: Arc<Props>,
    imports: Arc<HashMap<String, String>>,
    parent: Option<Layout>,
    caller: Option<Caller>,
    root: NodeId,
}

/// The file an instance is used in, and the attributes it lays over the
/// instance's root — `x`, `y`, `width` and the like — as given there.
#[derive(Debug, Clone)]
struct Caller {
    file: String,
    overrides: Vec<(parse::Attr, String)>,
}

/// Whether a value as written names a theme color.
fn writes_theme(v: &Value) -> bool {
    match v {
        Value::Theme(..) => true,
        Value::Str(s, _) => s.starts_with("linear-gradient(") && s.contains("theme."),
        Value::Array(items) => items.iter().any(writes_theme),
        Value::Object(fields) => fields.iter().any(|(_, v)| writes_theme(v)),
        _ => false,
    }
}

/// Every `themes.yml` a design file at `path` reads its themes from: its
/// folder's and each folder's above it.
fn themes_chain(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut folder = path.rsplit_once('/').map(|(f, _)| f);
    while let Some(f) = folder {
        out.push(format!("{f}/{THEMES_FILE}"));
        folder = f.rsplit_once('/').map(|(f, _)| f);
    }
    out.push(THEMES_FILE.to_string());
    out
}

/// The themes a folder's designs use: every `themes.yml` with `kind: themes`
/// from the top of the served folder down to it, merged — a lower file's
/// names over a higher one's, theme by theme.
#[derive(Debug, Clone, Default)]
struct Themes {
    /// Whether any such file was found.
    found: bool,
    light: BTreeMap<String, Rgba>,
    dark: BTreeMap<String, Rgba>,
    /// The fonts the files name, by name, a lower file's over a higher one's.
    fonts: BTreeMap<String, ThemesFont>,
}

/// A font a `themes.yml`'s `fonts` names: its path as written, its address —
/// `None` when it leaves the served folder — and where it is written.
#[derive(Debug, Clone)]
struct ThemesFont {
    written: String,
    at: Option<String>,
    origin: Origin,
}

/// One `themes.yml`'s two themes and the fonts it names, as far as they are
/// valid.
struct ThemesFile {
    light: BTreeMap<String, Rgba>,
    dark: BTreeMap<String, Rgba>,
    fonts: BTreeMap<String, ThemesFont>,
}

/// The file a folder's themes are in.
pub const THEMES_FILE: &str = "themes.yml";

/// How deep a resolved design's tree may nest, counting through instances:
/// the parser's own limit for one file, and so far beyond any real design.
const MAX_DEPTH: usize = parse::MAX_DEPTH;

/// A prop's value and the file that wrote it, which a path in it is
/// relative to.
#[derive(Debug, Clone)]
struct Given {
    value: Value,
    by: String,
}

type Props = HashMap<String, Given>;

/// A font a `font` names: its family, or why it can't be used; `None` when
/// there is nothing there.
type Found = Option<Result<Rc<Family>, String>>;

impl Resolver<'_> {
    /// The node being resolved reads the file `key` — a folder's listing
    /// when it ends in `/`: a change to it resolves the node again.
    fn read_now(&mut self, key: &str) {
        if let Some(node) = &self.current {
            let node = node.0.clone();
            self.record.read_by(key, &node);
        }
    }

    fn problem(&mut self, file: &str, pos: Pos, message: String) {
        self.problems.push(Problem {
            file: file.to_string(),
            pos,
            message,
        });
    }

    /// Binds every theme color written in `el` and the elements in it — in
    /// `path`'s file — to that file's themes.
    fn bind_element(&mut self, path: &str, el: &mut Element) {
        self.bind_own(path, el);
        for child in &mut el.children {
            if let Child::Element(e) = child {
                self.bind_element(path, e);
            }
        }
    }

    /// Binds the theme colors written in `el`'s own attributes, the node or
    /// instance it makes reading `path`'s themes when there are any.
    fn bind_own(&mut self, path: &str, el: &mut Element) {
        let themed = el.attrs.iter().any(|a| writes_theme(&a.value));
        let props_themed = el
            .attrs
            .iter()
            .any(|a| a.name == "props" && writes_theme(&a.value));
        for attr in &mut el.attrs {
            self.bind(path, &mut attr.value);
        }
        if themed {
            let mut id = self.instance.clone();
            id.push(el.id);
            for file in themes_chain(path) {
                self.record.read_by(&file, &id);
            }
        }
        // A theme color among a design's `props` defaults reaches whatever
        // reads that prop, anywhere in the design: the instance reads the
        // themes, so a change to them resolves it again with its subtree.
        if props_themed {
            let instance = self.instance.clone();
            for file in themes_chain(path) {
                self.record.read_by(&file, &instance);
            }
        }
    }

    /// Binds a value written in `path` to `path`'s themes: `theme.name` to
    /// its two colors, and a gradient naming theme colors to one gradient
    /// per theme.
    fn bind(&mut self, path: &str, value: &mut Value) {
        match value {
            Value::Theme(name, pos) => {
                // A problem already when there is none; any color keeps
                // the rest of the design checked.
                let (light, dark) = self
                    .theme_color(path, name, *pos)
                    .map_or(("#0000".into(), "#0000".into()), |(l, d)| (hex(l), hex(d)));
                *value = Value::Themed {
                    light,
                    dark,
                    written: format!("theme.{name}"),
                };
            }
            Value::Str(s, pos) if s.starts_with("linear-gradient(") && s.contains("theme.") => {
                let (mut light, mut dark) = (String::new(), String::new());
                let mut rest = s.as_str();
                let mut col = pos.col + 1;
                while let Some(at) = rest.find("theme.") {
                    let before = &rest[..at];
                    light.push_str(before);
                    dark.push_str(before);
                    col += before.chars().count() as u32;
                    let after = &rest[at + "theme.".len()..];
                    let len = after
                        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
                        .unwrap_or(after.len());
                    let name = &after[..len];
                    let at_ref = Pos {
                        line: pos.line,
                        col,
                    };
                    match self.theme_color(path, name, at_ref) {
                        Some((l, d)) => {
                            light.push_str(&hex(l));
                            dark.push_str(&hex(d));
                        }
                        // A problem already; any color keeps the rest checked.
                        None => {
                            light.push_str("#0000");
                            dark.push_str("#0000");
                        }
                    }
                    col += ("theme.".len() + name.chars().count()) as u32;
                    rest = &after[len..];
                }
                light.push_str(rest);
                dark.push_str(rest);
                *value = Value::Themed {
                    light,
                    dark,
                    written: format!("\"{s}\""),
                };
            }
            Value::Array(items) => {
                for item in items {
                    self.bind(path, item);
                }
            }
            Value::Object(fields) => {
                for (_, v) in fields {
                    self.bind(path, v);
                }
            }
            _ => {}
        }
    }

    /// The two colors of `theme.name`, written at `pos` in `path`, from the
    /// themes of `path`'s folder — or `None`, with the problem reported.
    fn theme_color(&mut self, path: &str, name: &str, pos: Pos) -> Option<(Rgba, Rgba)> {
        let folder = path.rsplit_once('/').map_or("", |(folder, _)| folder);
        let themes = self.themes(folder);
        let problem = if !themes.found {
            format!(
                "theme.{name}: no {THEMES_FILE} with kind: themes in this design's folder or above it"
            )
        } else {
            match (themes.light.get(name), themes.dark.get(name)) {
                (Some(l), Some(d)) => return Some((*l, *d)),
                (None, None) => {
                    let mut names: Vec<&String> =
                        themes.light.keys().chain(themes.dark.keys()).collect();
                    names.sort();
                    names.dedup();
                    let names: Vec<&str> = names.iter().map(|n| n.as_str()).collect();
                    if names.is_empty() {
                        format!("theme.{name}: the themes have no colors")
                    } else {
                        format!(
                            "theme.{name}: the themes have no {name}; they have {}",
                            names.join(", ")
                        )
                    }
                }
                (Some(_), None) => format!(
                    "theme.{name}: only the light theme has {name}; a name needs a color in both"
                ),
                (None, Some(_)) => format!(
                    "theme.{name}: only the dark theme has {name}; a name needs a color in both"
                ),
            }
        };
        self.problem(path, pos, problem);
        None
    }

    /// The themes the designs in `folder` use: its own `themes.yml` over
    /// those of the folders above it, each read once per resolve — a missing
    /// one too, so that creating it counts as a change.
    fn themes(&mut self, folder: &str) -> Rc<Themes> {
        if let Some(themes) = self.themes.get(folder) {
            return themes.clone();
        }
        let above = match folder.rsplit_once('/') {
            Some((parent, _)) => self.themes(parent),
            None if folder.is_empty() => Rc::new(Themes::default()),
            None => self.themes(""),
        };
        let file = if folder.is_empty() {
            THEMES_FILE.to_string()
        } else {
            format!("{folder}/{THEMES_FILE}")
        };
        let themes = match self.themes_file(&file) {
            None => above,
            Some(own) => {
                let mut merged = (*above).clone();
                merged.found = true;
                merged.light.extend(own.light);
                merged.dark.extend(own.dark);
                merged.fonts.extend(own.fonts);
                Rc::new(merged)
            }
        };
        self.themes.insert(folder.to_string(), themes.clone());
        themes
    }

    /// The themes in the file at `path`, as far as they are valid, with its
    /// problems reported; `None` when there is no such file or it isn't a
    /// themes file — its `kind` isn't `themes`.
    fn themes_file(&mut self, path: &str) -> Option<ThemesFile> {
        let text = self.files.read(path)?;
        if !self.read.iter().any(|p| p == path) {
            self.read.push(path.to_string());
        }
        let mut file = ThemesFile {
            light: BTreeMap::new(),
            dark: BTreeMap::new(),
            fonts: BTreeMap::new(),
        };
        let yaml: serde_yaml::Value = match serde_yaml::from_str(&text) {
            Ok(yaml) => yaml,
            Err(e) => {
                // Only a file that says it is a themes file is ours to report.
                if !text.lines().any(|l| l.trim() == "kind: themes") {
                    return None;
                }
                let pos = e.location().map_or(Pos { line: 1, col: 1 }, |l| Pos {
                    line: l.line() as u32,
                    col: l.column() as u32,
                });
                self.problem(path, pos, format!("this themes file isn't valid YAML: {e}"));
                return Some(file);
            }
        };
        let serde_yaml::Value::Mapping(top) = yaml else {
            return None;
        };
        if top.get("kind").and_then(|k| k.as_str()) != Some("themes") {
            return None;
        }
        let start = Pos { line: 1, col: 1 };
        for (key, value) in &top {
            let key = key.as_str().unwrap_or_default();
            let theme = match key {
                "kind" => continue,
                "light" => &mut file.light,
                "dark" => &mut file.dark,
                "fonts" => {
                    self.themes_fonts(path, &text, value, &mut file.fonts);
                    continue;
                }
                other => {
                    let pos = key_pos(&text, None, other).unwrap_or(start);
                    self.problem(
                        path,
                        pos,
                        format!("{other}: a themes file has kind, fonts, light and dark only"),
                    );
                    continue;
                }
            };
            let at = key_pos(&text, None, key).unwrap_or(start);
            let serde_yaml::Value::Mapping(colors) = value else {
                self.problem(
                    path,
                    at,
                    format!("{key}: expected a map of names to colors, like surface: \"#ffffff\""),
                );
                continue;
            };
            for (name, color) in colors {
                let Some(name) = name.as_str() else {
                    self.problem(path, at, format!("{key}: a color's name is text"));
                    continue;
                };
                let pos = key_pos(&text, Some(key), name).unwrap_or(at);
                if name.is_empty()
                    || name.starts_with(|c: char| c.is_ascii_digit() || c == '-')
                    || !name
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
                {
                    self.problem(
                        path,
                        pos,
                        format!(
                            "{key}.{name}: a name holds letters, digits, _ and -, and starts with a letter"
                        ),
                    );
                    continue;
                }
                let value = match color {
                    serde_yaml::Value::String(s) => rgba(s),
                    serde_yaml::Value::Null => Err(
                        "no color: write it in quotes, \"#rrggbb\", as YAML reads # as a comment"
                            .to_string(),
                    ),
                    _ => Err("expected a color like \"#rrggbb\"".to_string()),
                };
                match value {
                    Ok(c) => {
                        theme.insert(name.to_string(), c);
                    }
                    Err(e) => self.problem(path, pos, format!("{key}.{name}: {e}")),
                }
            }
        }
        for needed in ["light", "dark"] {
            if !top.contains_key(needed) {
                self.problem(
                    path,
                    start,
                    format!("a themes file has a light and a dark theme; this one has no {needed}"),
                );
            }
        }
        Some(file)
    }

    /// The fonts a `themes.yml` at `path` names in `value`, its `fonts`, each
    /// a name and a path from that file; a bad entry is a problem at its
    /// line.
    fn themes_fonts(
        &mut self,
        path: &str,
        text: &str,
        value: &serde_yaml::Value,
        into: &mut BTreeMap<String, ThemesFont>,
    ) {
        let start = Pos { line: 1, col: 1 };
        let at = key_pos(text, None, "fonts").unwrap_or(start);
        let serde_yaml::Value::Mapping(entries) = value else {
            self.problem(
                path,
                at,
                "fonts: expected a map of names to font files or folders, like figtree: ./fonts/Figtree".into(),
            );
            return;
        };
        for (name, written) in entries {
            let Some(name) = name.as_str() else {
                self.problem(path, at, "fonts: a font's name is text".into());
                continue;
            };
            let pos = key_pos(text, Some("fonts"), name).unwrap_or(at);
            if name.is_empty()
                || !name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            {
                self.problem(
                    path,
                    pos,
                    format!(
                        "fonts.{name}: a font's name holds lowercase letters, digits and - only"
                    ),
                );
                continue;
            }
            let Some(written) = written.as_str() else {
                self.problem(
                    path,
                    pos,
                    format!("fonts.{name}: expected a path to a font file or a folder of one family's, like ./fonts/Figtree"),
                );
                continue;
            };
            into.insert(
                name.to_string(),
                ThemesFont {
                    written: written.to_string(),
                    at: join(path, written),
                    origin: Origin {
                        file: path.to_string(),
                        pos,
                    },
                },
            );
        }
    }

    /// A design file's imports, each to the file it names, and its root's
    /// declared props — with the problems of either reported.
    /// `None` when its root isn't a `Frame`.
    fn scope(
        &mut self,
        path: &str,
        file: &parse::File,
    ) -> Option<(HashMap<String, String>, Props)> {
        let mut imports = HashMap::new();
        for import in &file.imports {
            if import.name == parse::THEME {
                self.problem(
                    path,
                    import.pos,
                    format!(
                        "import {0} — {0} is the name of the design's themes",
                        import.name
                    ),
                );
                continue;
            }
            if ELEMENTS.contains(&import.name.as_str()) {
                self.problem(
                    path,
                    import.pos,
                    format!("import {0} — {0} is a built-in element", import.name),
                );
                continue;
            }
            if imports.contains_key(&import.name) {
                self.problem(
                    path,
                    import.pos,
                    format!("import {0} — {0} is already imported", import.name),
                );
                continue;
            }
            if !import.path.ends_with(".design") {
                self.problem(
                    path,
                    import.pos,
                    format!("import {} — a design is a .design file", import.path),
                );
                continue;
            }
            match join(path, &import.path) {
                Some(target) => {
                    // Every import must exist, used or not.
                    if self.files.read(&target).is_none() {
                        self.problem(
                            path,
                            import.pos,
                            format!("import {} — no such file", import.path),
                        );
                        self.missing.push(target.clone());
                    }
                    imports.insert(import.name.clone(), target);
                }
                None => self.problem(
                    path,
                    import.pos,
                    format!("import {} — outside the served folder", import.path),
                ),
            }
        }
        if file.root.name != "Frame" {
            self.problem(
                path,
                file.root.pos,
                format!("the root element must be a Frame, not {}", file.root.name),
            );
            return None;
        }
        // Props: the root's declared defaults, overridden by the instance's.
        let mut props = Props::new();
        if let Some(attr) = file.root.attrs.iter().find(|a| a.name == "props") {
            match &attr.value {
                Value::Object(fields) => {
                    for (k, v) in fields {
                        if k == parse::THEME {
                            self.problem(
                                path,
                                attr.pos,
                                format!(
                                    "props: {k} is the name of the design's themes, not a prop"
                                ),
                            );
                            continue;
                        }
                        props.insert(
                            k.clone(),
                            Given {
                                value: v.clone(),
                                by: path.to_string(),
                            },
                        );
                    }
                }
                _ => self.problem(
                    path,
                    attr.pos,
                    "props: expected an object of defaults".into(),
                ),
            }
        }
        Some((imports, props))
    }

    /// The root node of the design at `path`, with `args` given to its props.
    fn design(
        &mut self,
        path: &str,
        args: &[(parse::Attr, String)],
        from: Option<(&str, Pos)>,
        used_as: &str,
        parent: Option<Layout>,
    ) -> Option<Node> {
        if self.stack.iter().any(|p| p == path) {
            let (file, pos) = from.unwrap_or((path, Pos { line: 1, col: 1 }));
            self.problem(file, pos, format!("{path} uses itself"));
            return None;
        }
        let Some(source) = self.files.read(path) else {
            if from.is_some() && self.missing.iter().any(|p| p == path) {
                return None;
            }
            let (file, pos) = from.unwrap_or((path, Pos { line: 1, col: 1 }));
            self.problem(file, pos, format!("{path} — no such file"));
            return None;
        };
        if !self.read.iter().any(|p| p == path) {
            self.read.push(path.to_string());
        }
        let mut file = match self.files.parse(path, &source) {
            Ok(file) => {
                self.record.parses.insert(path.to_string(), file.clone());
                (*file).clone()
            }
            Err(mut problem) => {
                problem.file = path.to_string();
                self.problems.push(problem);
                return None;
            }
        };
        // Bound now, before any value leaves this file as a prop: a theme
        // color is this file's, wherever it is used.
        self.bind_element(path, &mut file.root);
        let (imports, mut props) = self.scope(path, &file)?;
        let (caller, caller_pos) = from.unwrap_or((path, file.root.pos));
        let caller = caller.to_string();
        let mut overrides = Vec::new();
        for (arg, by) in args {
            if matches!(
                arg.name.as_str(),
                "x" | "y" | "width" | "height" | "name" | "opacity" | "onClick"
            ) {
                overrides.push((arg.clone(), by.clone()));
            } else if props.contains_key(&arg.name) {
                props.insert(
                    arg.name.clone(),
                    Given {
                        value: arg.value.clone(),
                        by: by.clone(),
                    },
                );
            } else {
                self.problem(
                    &caller,
                    arg.pos,
                    format!("{used_as} has no prop \"{}\"", arg.name),
                );
            }
        }
        let _ = caller_pos;
        let key = self.instance.clone();
        for target in imports.values() {
            self.record.read_by(target, &key);
        }
        self.record.instances.insert(
            key.clone(),
            Instance {
                file: path.to_string(),
                props: Arc::new(props.clone()),
                imports: Arc::new(imports.clone()),
                parent,
                caller: from.map(|_| Caller {
                    file: caller.clone(),
                    overrides: overrides.clone(),
                }),
                root: {
                    let mut id = key.clone();
                    id.push(file.root.id);
                    NodeId(id)
                },
            },
        );
        self.record
            .by_file
            .entry(path.to_string())
            .or_default()
            .insert(key);
        self.stack.push(path.to_string());
        let mut root = self.element(path, &file.root, &props, &imports, true, parent);
        self.stack.pop();
        if let Some(root) = root.as_mut() {
            self.current = Some(root.id.clone());
            for (attr, by) in &overrides {
                // The caller's own props are already substituted into these.
                self.common(&caller, by, root, attr, attr.value.clone());
                self.fill_needs_layout(&caller, attr, &attr.value, parent);
                self.place_needs_no_layout(&caller, attr, parent);
            }
        }
        root
    }

    fn element(
        &mut self,
        path: &str,
        el: &Element,
        props: &Props,
        imports: &HashMap<String, String>,
        root: bool,
        parent: Option<Layout>,
    ) -> Option<Node> {
        if self.depth >= MAX_DEPTH {
            self.problem(path, el.pos, format!("nested more than {MAX_DEPTH} deep"));
            return None;
        }
        self.depth += 1;
        let node = self.element_inner(path, el, props, imports, root, parent);
        self.depth -= 1;
        node
    }

    fn element_inner(
        &mut self,
        path: &str,
        el: &Element,
        props: &Props,
        imports: &HashMap<String, String>,
        root: bool,
        parent: Option<Layout>,
    ) -> Option<Node> {
        // An attribute given twice is a problem, on an element or an instance
        // alike; the first one counts.
        let mut seen: Vec<&str> = Vec::new();
        let mut attrs = Vec::new();
        for attr in &el.attrs {
            if seen.contains(&attr.name.as_str()) {
                self.problem(path, attr.pos, format!("{} is given twice", attr.name));
            } else {
                seen.push(&attr.name);
                attrs.push(attr);
            }
        }
        if let Some(target) = imports.get(&el.name) {
            if !el.children.is_empty() {
                self.problem(path, el.pos, format!("{} takes no children", el.name));
            }
            let args: Vec<(parse::Attr, String)> = attrs
                .iter()
                .filter(|a| a.name != "key" || !self.key_ok(path, a))
                .map(|a| {
                    let (value, by) = substitute(&a.value, props, path);
                    let arg = parse::Attr {
                        name: a.name.clone(),
                        value,
                        pos: a.pos,
                    };
                    (arg, by)
                })
                .collect();
            let target = target.clone();
            self.instance.push(el.id);
            let node = self.design(&target, &args, Some((path, el.pos)), &el.name, parent);
            self.instance.pop();
            return node;
        }
        let kind = match el.name.as_str() {
            // Keep in step with ELEMENTS.
            "Frame" => Kind::Frame,
            "Text" => Kind::Text,
            "Rect" => Kind::Rect,
            "Ellipse" => Kind::Ellipse,
            "Vector" => Kind::Vector,
            "Image" => Kind::Image,
            other => {
                self.problem(path, el.pos, format!("unknown element {other}"));
                return None;
            }
        };
        let mut node = Node::new(kind, &el.name);
        let mut id = self.instance.clone();
        id.push(el.id);
        node.id = NodeId(id);
        self.current = Some(node.id.clone());
        // A text's `font`, as given and by the file that wrote it, chosen a
        // face from once its weight is known.
        let mut font = None;
        for &attr in &attrs {
            if attr.name == "props" {
                if !root {
                    self.problem(
                        path,
                        attr.pos,
                        "props: only a design's root declares props".into(),
                    );
                }
                continue;
            }
            // Shown on its own a design has no parent to place it in, and used
            // as an instance it goes where the instance's own x and y say.
            if root && matches!(attr.name.as_str(), "x" | "y") {
                self.problem(
                    path,
                    attr.pos,
                    format!(
                        "{}: a design's root has no place of its own; place an instance with x and y where it is used",
                        attr.name
                    ),
                );
                continue;
            }
            let (value, by) = match &attr.value {
                Value::Prop(p) => match props.get(p) {
                    Some(given) => (given.value.clone(), given.by.clone()),
                    None => {
                        self.problem(path, attr.pos, format!("{}: no prop \"{p}\"", attr.name));
                        continue;
                    }
                },
                v => (v.clone(), path.to_string()),
            };
            if has_theme(&value) {
                self.themed = true;
                self.record.themed.insert(node.id.clone());
            }
            self.fill_needs_layout(path, attr, &value, parent);
            self.place_needs_no_layout(path, attr, parent);
            if kind == Kind::Text && attr.name == "font" {
                font = Some((attr, value, by));
                continue;
            }
            self.attribute(path, &by, &mut node, attr, value);
        }
        match kind {
            Kind::Vector if !el.attrs.iter().any(|a| a.name == "d") => self.problem(
                path,
                el.pos,
                format!("{} needs a path, d=\"M…\"", node.name),
            ),
            Kind::Image if node.image.is_none() && !el.attrs.iter().any(|a| a.name == "src") => {
                self.problem(
                    path,
                    el.pos,
                    format!("{} needs a file, src=\"…\"", node.name),
                )
            }
            _ => {}
        }
        if kind == Kind::Text {
            let mut content = String::new();
            for child in &el.children {
                match child {
                    Child::Text(parts, pos) => {
                        for part in parts {
                            match part {
                                TextPart::Literal(s) => content.push_str(s),
                                TextPart::Prop(p) => match props.get(p).map(|g| &g.value) {
                                    Some(Value::Str(s, _)) => content.push_str(s),
                                    Some(Value::Num(n)) => content.push_str(&n.to_string()),
                                    _ => self.problem(path, *pos, format!("no text prop \"{p}\"")),
                                },
                                TextPart::Theme(name, at) => self.problem(
                                    path,
                                    *at,
                                    format!(
                                        "theme.{name} is a color, not text: give it to color={{theme.{name}}}"
                                    ),
                                ),
                            }
                        }
                    }
                    Child::Element(e) => self.problem(path, e.pos, "Text holds text only".into()),
                }
            }
            let text = node.text.get_or_insert_with(default_text);
            text.content = content;
            let weight = text.weight;
            let weight_at = attrs.iter().find(|a| a.name == "weight").map(|a| a.pos);
            text.font = self.text_font(path, el.pos, font, weight, weight_at);
        } else {
            let mut keys: HashMap<&str, Pos> = HashMap::new();
            for child in &el.children {
                if let Child::Element(e) = child
                    && let Some(k) = crate::ids::key(e)
                    && keys.insert(k, e.pos).is_some()
                {
                    let at = e
                        .attrs
                        .iter()
                        .find(|a| a.name == "key")
                        .map_or(e.pos, |a| a.pos);
                    self.problem(path, at, format!("key: {k:?} is another sibling's key too"));
                }
                match child {
                    Child::Element(_) if self.own_only => {}
                    Child::Element(e) => {
                        if kind != Kind::Frame {
                            self.problem(path, e.pos, format!("{} takes no children", el.name));
                        } else if let Some(n) =
                            self.element(path, e, props, imports, false, Some(node.layout))
                        {
                            node.children.push(n);
                            self.current = Some(node.id.clone());
                        }
                    }
                    Child::Text(_, pos) => self.problem(
                        path,
                        *pos,
                        format!("text belongs in a Text, not a {}", el.name),
                    ),
                }
            }
        }
        Some(node)
    }

    /// `"fill"` takes the free space of a parent's auto layout, so a child of a
    /// frame with no layout can't have it.
    fn fill_needs_layout(
        &mut self,
        path: &str,
        attr: &parse::Attr,
        value: &Value,
        parent: Option<Layout>,
    ) {
        if parent == Some(Layout::None)
            && matches!(attr.name.as_str(), "width" | "height")
            && matches!(value, Value::Str(s, _) if s == "fill")
        {
            self.problem(
                path,
                attr.pos,
                format!(
                    "{}: \"fill\" needs a parent with layout=\"row\" or \"column\"",
                    attr.name
                ),
            );
        }
    }

    /// `x` and `y` place a child inside a frame with no layout; auto layout
    /// places its children itself, so there they would mean nothing.
    fn place_needs_no_layout(&mut self, path: &str, attr: &parse::Attr, parent: Option<Layout>) {
        if matches!(parent, Some(Layout::Row | Layout::Column))
            && matches!(attr.name.as_str(), "x" | "y")
        {
            self.problem(
                path,
                attr.pos,
                format!(
                    "{}: places a child only in a frame with layout=\"none\"",
                    attr.name
                ),
            );
        }
    }

    // Each arm sets a field and says whether the value fit: `Ok(field = v)`.
    #[allow(clippy::unit_arg)]
    fn attribute(
        &mut self,
        path: &str,
        by: &str,
        node: &mut Node,
        attr: &parse::Attr,
        value: Value,
    ) {
        let kind = node.kind;
        // A path is relative to the file that wrote it, and its problem is
        // where it was written.
        let (at_file, at_pos) = written_at(path, by, attr, &value);
        let ok = match (attr.name.as_str(), kind) {
            ("x" | "y" | "width" | "height" | "name" | "opacity" | "onClick", _) => {
                self.common(path, by, node, attr, value);
                return;
            }
            ("key", _) => {
                self.key_ok(path, attr);
                return;
            }
            ("fill", k) if k != Kind::Text => fill(&value).map(|f| node.fill = Some(f)),
            ("stroke", k) if k != Kind::Text => color(&value).map(|c| node.stroke = Some(c)),
            ("strokeWidth", k) if k != Kind::Text => px(&value).map(|n| node.stroke_width = n),
            ("radius", k) if k != Kind::Text => match &value {
                Value::Num(_) => px(&value).map(|n| node.radius = [n; 4]),
                Value::Array(a) if a.len() == 4 => {
                    pxs(a).map(|v| node.radius = [v[0], v[1], v[2], v[3]])
                }
                _ => Err("expected pixels or an array of 4".into()),
            },
            ("shadow", k) if k != Kind::Text => shadow(&value).map(|mut s| {
                s.at = Origin {
                    file: path.to_string(),
                    pos: attr.pos,
                };
                node.shadow = Some(s)
            }),
            // Only a frame has children to clip.
            ("clip", Kind::Frame) => match value {
                Value::Bool(b) => Ok(node.clip = b),
                _ => Err("expected true or false".into()),
            },
            ("layout", Kind::Frame) => match str_of(&value).as_deref() {
                Some("row") => Ok(node.layout = Layout::Row),
                Some("column") => Ok(node.layout = Layout::Column),
                Some("none") => Ok(node.layout = Layout::None),
                _ => Err("expected \"row\", \"column\" or \"none\"".into()),
            },
            ("gap", Kind::Frame) => px(&value).map(|n| node.gap = n),
            ("padding", Kind::Frame) => match &value {
                Value::Num(_) => px(&value).map(|n| node.padding = [n; 4]),
                Value::Array(a) if a.len() == 2 => {
                    pxs(a).map(|v| node.padding = [v[0], v[1], v[0], v[1]])
                }
                Value::Array(a) if a.len() == 4 => {
                    pxs(a).map(|v| node.padding = [v[0], v[1], v[2], v[3]])
                }
                other => Err(format!(
                    "expected pixels or an array of 2 or 4, found {}",
                    show(other)
                )),
            },
            ("align", Kind::Frame) => align(&value, false).map(|a| node.align = a),
            ("justify", Kind::Frame) => align(&value, true).map(|a| node.justify = a),
            ("size", Kind::Text) => {
                px(&value).map(|n| node.text.get_or_insert_with(default_text).size = n)
            }
            ("weight", Kind::Text) => match num(&value) {
                Ok(w) if fonts::WEIGHTS.iter().any(|&k| k as f32 == w) => {
                    Ok(node.text.get_or_insert_with(default_text).weight = w as u16)
                }
                _ => Err("expected 100 to 900 in steps of 100".into()),
            },
            ("color", Kind::Text) => {
                color(&value).map(|c| node.text.get_or_insert_with(default_text).color = c)
            }
            ("lineHeight", Kind::Text) => {
                px(&value).map(|n| node.text.get_or_insert_with(default_text).line_height = n)
            }
            ("textAlign", Kind::Text) => match str_of(&value).as_deref() {
                Some("left") => {
                    Ok(node.text.get_or_insert_with(default_text).align = TextAlign::Left)
                }
                Some("center") => {
                    Ok(node.text.get_or_insert_with(default_text).align = TextAlign::Center)
                }
                Some("right") => {
                    Ok(node.text.get_or_insert_with(default_text).align = TextAlign::Right)
                }
                _ => Err("expected \"left\", \"center\" or \"right\"".into()),
            },
            ("d", Kind::Vector) => match str_of(&value) {
                Some(d) => match path_numbers(&d).map_err(|e| e.to_string()).and_then(|()| {
                    kurbo::BezPath::from_svg(&d).map_err(|e| format!("not SVG path data: {e}"))
                }) {
                    Ok(_) => {
                        let view_box = node.vector.as_ref().map(|v| v.view_box);
                        let view_box = view_box.unwrap_or_else(|| bounds(&d));
                        Ok(node.vector = Some(Vector { d, view_box }))
                    }
                    Err(e) => Err(e),
                },
                None => Err("expected SVG path data".into()),
            },
            ("viewBox", Kind::Vector) => match str_of(&value).as_deref().map(view_box) {
                Some(Ok(b)) => {
                    let v = node.vector.get_or_insert_with(|| Vector {
                        d: String::new(),
                        view_box: b,
                    });
                    Ok(v.view_box = b)
                }
                Some(Err(e)) => Err(e),
                None => Err("expected \"minX minY width height\"".into()),
            },
            ("src", Kind::Image) => match str_of(&value) {
                Some(src) => match join(by, &src) {
                    None => {
                        let e = format!("src: {src} — outside the served folder");
                        self.problem(&at_file, at_pos, e);
                        return;
                    }
                    Some(at) => match self.image_file(&at) {
                        None => {
                            self.problem(&at_file, at_pos, format!("src: {src} — no such file"));
                            return;
                        }
                        Some(Err(e)) => {
                            self.problem(&at_file, at_pos, format!("src: {src} {e}"));
                            return;
                        }
                        Some(Ok(id)) => {
                            node.image = Some(Image {
                                src: at,
                                id,
                                at: Origin {
                                    file: path.to_string(),
                                    pos: attr.pos,
                                },
                            });
                            Ok(())
                        }
                    },
                },
                None => Err("expected a path".into()),
            },
            (name, _) => {
                self.problem(
                    path,
                    attr.pos,
                    format!("{} takes no attribute {name}", node.name),
                );
                return;
            }
        };
        if let Err(e) = ok {
            self.problem(path, attr.pos, format!("{}: {e}", attr.name));
        }
    }

    /// The image file at `at`, by its content — read and decoded once per
    /// resolve however many `Image`s show it — or why it doesn't decode;
    /// `None` when there is no such file.
    fn image_file(&mut self, at: &str) -> Option<Result<ImageId, String>> {
        self.read_now(at);
        if let Some(known) = self.by_path.get(at) {
            return Some(known.clone());
        }
        let file = self.files.image(at)?;
        if !self.read.iter().any(|p| p == at) {
            self.read.push(at.to_string());
        }
        let id = file.map(|file| {
            let id = file.id;
            self.images.entry(id).or_insert(file);
            id
        });
        self.by_path.insert(at.to_string(), id.clone());
        Some(id)
    }

    /// The font file a `Text` written at `pos` in `path` is set in, for its
    /// `weight` — `None` for Inter, when it has no `font`. Its `font`, given
    /// as `value` by the file `by`, is a name the themes of `by`'s folder
    /// give a font. An unknown name is a problem where it is written; a font
    /// that can't be used, at its entry in the `themes.yml`, once; a weight
    /// the font lacks, at the `weight` — or the `Text`.
    fn text_font(
        &mut self,
        path: &str,
        pos: Pos,
        font: Option<(&parse::Attr, Value, String)>,
        weight: u16,
        weight_at: Option<Pos>,
    ) -> Option<FontId> {
        let Some((attr, value, by)) = font else {
            if !fonts::INTER_WEIGHTS.contains(&weight) {
                self.no_weight(
                    path,
                    pos,
                    weight_at,
                    weight,
                    fonts::INTER,
                    &fonts::INTER_WEIGHTS,
                );
            }
            return None;
        };
        let (file, at_pos) = written_at(path, &by, attr, &value);
        let Some(name) = str_of(&value) else {
            self.problem(
                &file,
                at_pos,
                "font: expected the name of a font the themes name, like \"figtree\"".into(),
            );
            return None;
        };
        let folder = by.rsplit_once('/').map_or("", |(folder, _)| folder);
        for file in themes_chain(&by) {
            self.read_now(&file);
        }
        let named = self.themes(folder).fonts.clone();
        let Some(entry) = named.get(&name) else {
            let problem = if named.is_empty() {
                format!(
                    "font: no font named {name}; no {THEMES_FILE} in this design's folder or above it names fonts"
                )
            } else {
                let names: Vec<&str> = named.keys().map(String::as_str).collect();
                format!(
                    "font: no font named {name}; the themes name {}",
                    names.join(", ")
                )
            };
            self.problem(&file, at_pos, problem);
            return None;
        };
        let origin = entry.origin.clone();
        let Some(at) = entry.at.clone() else {
            let key = format!("{}\0{}", origin.file, name);
            if let std::collections::hash_map::Entry::Vacant(first) = self.families.entry(key) {
                first.insert((None, true));
                self.problem(
                    &origin.file,
                    origin.pos,
                    format!(
                        "fonts.{name}: {} — outside the served folder",
                        entry.written
                    ),
                );
            }
            return None;
        };
        self.read_now(&at);
        self.read_now(&format!("{at}/"));
        let (family, reported) = self.family(&at);
        let family = match family {
            Some(Ok(family)) => family,
            missing_or_bad => {
                if !reported {
                    let problem = match missing_or_bad {
                        Some(Err(e)) => format!("fonts.{name}: {} {e}", entry.written),
                        _ => format!("fonts.{name}: {} — no such file or folder", entry.written),
                    };
                    self.problem(&origin.file, origin.pos, problem);
                    if let Some(known) = self.families.get_mut(&at) {
                        known.1 = true;
                    }
                }
                return None;
            }
        };
        match family.face(weight) {
            Some(face) => {
                self.fonts.entry(face.id).or_insert_with(|| face.clone());
                Some(face.id)
            }
            None => {
                let has = family.weights();
                self.no_weight(path, pos, weight_at, weight, &family.name, &has);
                None
            }
        }
    }

    /// The problem of a weight the font doesn't have: at the `Text`'s
    /// `weight`, or at the `Text` when it leaves the weight at 400.
    fn no_weight(
        &mut self,
        path: &str,
        pos: Pos,
        weight_at: Option<Pos>,
        weight: u16,
        family: &str,
        has: &[u16],
    ) {
        let message = if has.is_empty() {
            format!(
                "weight: {family} has no {weight}; it has no weight from 100 to 900 in steps of 100"
            )
        } else {
            format!(
                "weight: {family} has no {weight}; it has {}",
                fonts::list(has)
            )
        };
        self.problem(path, weight_at.unwrap_or(pos), message);
    }

    /// The font at `at` — one file, or a folder of one family's files — read
    /// and checked once per resolve, or why it can't be used; `None` when
    /// there is nothing there. Also whether its problem was reported yet.
    fn family(&mut self, at: &str) -> (Found, bool) {
        if let Some(known) = self.families.get(at) {
            return known.clone();
        }
        let family = match self.font_file(at) {
            Some(Ok(file)) => Some(Ok(Rc::new(Family {
                name: file.family.clone(),
                faces: vec![file],
            }))),
            Some(Err(e)) => Some(Err(e)),
            None => self.files.list(at).map(|names| {
                let mut faces = Vec::new();
                for name in names.into_iter().filter(|n| fonts::is_font_name(n)) {
                    match self.font_file(&format!("{at}/{name}")) {
                        Some(Ok(file)) => faces.push((name, file)),
                        Some(Err(e)) => return Err(format!("holds {name}, which {e}")),
                        // Gone since the folder was listed.
                        None => {}
                    }
                }
                Family::of_folder(faces).map(Rc::new)
            }),
        };
        self.families
            .insert(at.to_string(), (family.clone(), false));
        (family, false)
    }

    /// The font file at `at`, checked, or why it isn't one; `None` when there
    /// is no such file.
    fn font_file(&mut self, at: &str) -> Option<Result<Arc<FontFile>, String>> {
        let file = self.files.font(at)?;
        if !self.read.iter().any(|p| p == at) {
            self.read.push(at.to_string());
        }
        Some(file)
    }

    /// An interaction with its path, if it has one, made relative to the top of
    /// the served folder instead of to `path`, the file it is written in.
    fn interaction(&mut self, path: &str, action: &str) -> Result<String, String> {
        let Some((verb, target)) = action.split_once(' ') else {
            return Ok(action.to_string());
        };
        let target = target.trim();
        if !target.ends_with(".design") {
            return Err(format!("{action} — a design is a .design file"));
        }
        match join(path, target) {
            None => Err(format!("{action} — outside the served folder")),
            Some(at)
                if {
                    self.read_now(&at);
                    self.files.bytes(&at).is_none()
                } =>
            {
                Err(format!("{action} — no such file"))
            }
            Some(at) => Ok(format!("{verb} {at}")),
        }
    }

    /// Whether `attr`, a `key`, is a string — the one value a parse can
    /// match siblings by — saying so where it isn't. True either way once
    /// it has been looked at, so an instance doesn't pass it on as a prop.
    fn key_ok(&mut self, path: &str, attr: &parse::Attr) -> bool {
        if !matches!(attr.value, Value::Str(..)) {
            self.problem(path, attr.pos, "key: expected a string".into());
        }
        true
    }

    fn common(&mut self, path: &str, by: &str, node: &mut Node, attr: &parse::Attr, value: Value) {
        let (at_file, at_pos) = written_at(path, by, attr, &value);
        let ok = match attr.name.as_str() {
            "x" => num(&value).map(|n| node.x = n),
            "y" => num(&value).map(|n| node.y = n),
            "width" => size(&value).map(|s| node.width = s),
            "height" => size(&value).map(|s| node.height = s),
            "name" => str_of(&value)
                .map(|s| node.name = s)
                .ok_or_else(|| "expected a string".into()),
            "opacity" => match value {
                Value::Num(n) if (0.0..=1.0).contains(&n) => {
                    node.opacity = n as f32;
                    Ok(())
                }
                other => Err(format!(
                    "expected a number from 0 to 1, found {}",
                    show(&other)
                )),
            },
            "onClick" => match str_of(&value).filter(|s| interaction_ok(s)) {
                Some(action) => match self.interaction(by, &action) {
                    Ok(a) => {
                        node.on_click = Some(a);
                        Ok(())
                    }
                    Err(e) => {
                        self.problem(&at_file, at_pos, format!("onClick: {e}"));
                        return;
                    }
                },
                None => Err(
                    "expected \"navigate <path>\", \"back\", \"overlay <path>\" or \"close\""
                        .into(),
                ),
            },
            _ => unreachable!(),
        };
        if let Err(e) = ok {
            self.problem(path, attr.pos, format!("{}: {e}", attr.name));
        }
    }
}

fn default_text() -> TextStyle {
    TextStyle {
        font: None,
        content: String::new(),
        size: 14.0,
        weight: 400,
        color: Color::fixed(Rgba {
            r: 0x0f,
            g: 0x17,
            b: 0x2a,
            a: 255,
        }),
        line_height: 1.4,
        align: TextAlign::Left,
    }
}

fn view_box(s: &str) -> Result<[f32; 4], String> {
    let v: Vec<f32> = s
        .split([' ', ','])
        .filter(|p| !p.is_empty())
        .map(|p| parse::number(p).map(|n| n as f32))
        .collect::<Result<_, _>>()?;
    match v[..] {
        [x, y, w, h] if w > 0.0 && h > 0.0 => Ok([x, y, w, h]),
        _ => Err("expected \"minX minY width height\"".into()),
    }
}

/// Every number in SVG path data `d` held to the same limits as a design's
/// other numbers. A number here is SVG's — it may have an exponent — and an
/// arc's two flags are single digits that may be written run together.
fn path_numbers(d: &str) -> Result<(), String> {
    let b = d.as_bytes();
    let mut i = 0;
    let mut command = b' ';
    // Which parameter of the current command comes next.
    let mut param = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_alphabetic() && c != b'e' && c != b'E' {
            command = c;
            param = 0;
            i += 1;
            continue;
        }
        if c.is_ascii_whitespace() || c == b',' {
            i += 1;
            continue;
        }
        if matches!(command, b'A' | b'a') && matches!(param % 7, 3 | 4) && matches!(c, b'0' | b'1')
        {
            param += 1;
            i += 1;
            continue;
        }
        let start = i;
        if matches!(b[i], b'+' | b'-') {
            i += 1;
        }
        let mut dot = false;
        while i < b.len() && (b[i].is_ascii_digit() || (b[i] == b'.' && !dot)) {
            dot |= b[i] == b'.';
            i += 1;
        }
        if i < b.len() && matches!(b[i], b'e' | b'E') {
            let mut j = i + 1;
            if j < b.len() && matches!(b[j], b'+' | b'-') {
                j += 1;
            }
            if j < b.len() && b[j].is_ascii_digit() {
                i = j;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
        }
        if i == start {
            // Not a number: the path parser says what is wrong.
            return Ok(());
        }
        let s = &d[start..i];
        if let Ok(n) = s.parse::<f64>() {
            parse::in_range(s, n)?;
        }
        param += 1;
    }
    Ok(())
}

/// A path's own bounds, the view box of a `Vector` that names none.
fn bounds(d: &str) -> [f32; 4] {
    use kurbo::Shape;
    let b = kurbo::BezPath::from_svg(d)
        .map(|p| p.bounding_box())
        .unwrap_or_default();
    [
        b.x0 as f32,
        b.y0 as f32,
        (b.width() as f32).max(1.0),
        (b.height() as f32).max(1.0),
    ]
}

fn interaction_ok(s: &str) -> bool {
    s == "back"
        || s == "close"
        || s.strip_prefix("navigate ")
            .or(s.strip_prefix("overlay "))
            .is_some_and(|p| !p.trim().is_empty())
}

/// A path relative to a design's file, from the top of the served folder, or
/// `None` when it climbs out of the served folder.
pub fn join(from: &str, relative: &str) -> Option<String> {
    let mut parts: Vec<&str> = from.split('/').filter(|p| !p.is_empty()).collect();
    parts.pop();
    for part in relative.split('/') {
        match part {
            "." | "" => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    Some(parts.join("/"))
}

/// `value`, written in `path`, with a prop put in its place, and the file
/// that wrote what it ends up as.
fn substitute(value: &Value, props: &Props, path: &str) -> (Value, String) {
    match value {
        Value::Prop(p) => match props.get(p) {
            Some(given) => (given.value.clone(), given.by.clone()),
            None => (value.clone(), path.to_string()),
        },
        v => (v.clone(), path.to_string()),
    }
}

/// Where a path in `attr`'s `value` was written: at the attribute, in
/// `path`, or — when the value came as a prop from another file, `by` — at
/// the string there.
fn written_at(path: &str, by: &str, attr: &parse::Attr, value: &Value) -> (String, Pos) {
    match value {
        Value::Str(_, pos) if by != path => (by.to_string(), *pos),
        _ => (path.to_string(), attr.pos),
    }
}

fn show(v: &Value) -> String {
    match v {
        Value::Str(s, _) => format!("\"{s}\""),
        Value::Num(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Array(_) => "an array".into(),
        Value::Object(_) => "an object".into(),
        Value::Prop(p) => p.clone(),
        Value::Theme(name, _) => format!("theme.{name}"),
        Value::Themed { written, .. } => written.clone(),
    }
}

fn str_of(v: &Value) -> Option<String> {
    match v {
        Value::Str(s, _) => Some(s.clone()),
        _ => None,
    }
}

/// Where `key` is written in a themes file's text: at the top level, or —
/// with `under` — inside that top-level key's block. YAML's values carry no
/// place, so a key is found by its text: on a line of its own, plain or in
/// quotes.
fn key_pos(text: &str, under: Option<&str>, key: &str) -> Option<Pos> {
    let names = |line: &str, key: &str| {
        let line = line.trim_start();
        [
            format!("{key}:"),
            format!("\"{key}\":"),
            format!("'{key}':"),
        ]
        .iter()
        .any(|k| line.starts_with(k.as_str()))
    };
    let top = |line: &str| !line.starts_with([' ', '\t']) && !line.trim().is_empty();
    let mut inside = under.is_none();
    for (n, line) in text.lines().enumerate() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        let pos = Pos {
            line: n as u32 + 1,
            col: (line.len() - line.trim_start().len()) as u32 + 1,
        };
        match under {
            None if top(line) && names(line, key) => return Some(pos),
            None => {}
            Some(block) if top(line) => inside = names(line, block),
            Some(_) if inside && names(line, key) => return Some(pos),
            Some(_) => {}
        }
    }
    None
}

/// The built-in elements; an import can't take one of their names.
const ELEMENTS: [&str; 6] = ["Frame", "Text", "Rect", "Ellipse", "Vector", "Image"];

fn num(v: &Value) -> Result<f32, String> {
    match v {
        Value::Num(n) => Ok(*n as f32),
        other => Err(format!("expected a number, found {}", show(other))),
    }
}

/// A length or a multiple that can't be negative: pixels, a size, a line height.
fn px(v: &Value) -> Result<f32, String> {
    match v {
        Value::Num(n) if *n >= 0.0 => Ok(*n as f32),
        Value::Num(n) => Err(format!("expected a number of 0 or more, found {n}")),
        other => Err(format!("expected a number, found {}", show(other))),
    }
}

fn pxs(a: &[Value]) -> Result<Vec<f32>, String> {
    a.iter().map(px).collect()
}

fn size(v: &Value) -> Result<Size, String> {
    match v {
        Value::Num(n) if *n >= 0.0 => Ok(Size::Px(*n as f32)),
        Value::Num(n) => Err(format!("expected pixels of 0 or more, found {n}")),
        Value::Str(s, _) if s == "hug" => Ok(Size::Hug),
        Value::Str(s, _) if s == "fill" => Ok(Size::Fill),
        other => Err(format!(
            "expected pixels, \"hug\" or \"fill\", found {}",
            show(other)
        )),
    }
}

fn align(v: &Value, justify: bool) -> Result<Align, String> {
    match str_of(v).as_deref() {
        Some("start") => Ok(Align::Start),
        Some("center") => Ok(Align::Center),
        Some("end") => Ok(Align::End),
        Some("space-between") if justify => Ok(Align::SpaceBetween),
        _ if justify => Err("expected \"start\", \"center\", \"end\" or \"space-between\"".into()),
        _ => Err("expected \"start\", \"center\" or \"end\"".into()),
    }
}

/// A color: a `#…` one, the same in both themes, or a theme color bound to
/// its file's themes.
fn color(v: &Value) -> Result<Color, String> {
    match v {
        Value::Str(s, _) => rgba(s).map(Color::fixed),
        Value::Themed { light, dark, .. } => Ok(Color {
            light: rgba(light)?,
            dark: rgba(dark)?,
        }),
        _ => Err(format!("expected a color, found {}", show(v))),
    }
}

/// A `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa` color.
pub fn rgba(s: &str) -> Result<Rgba, String> {
    if s.starts_with("theme.") {
        return Err(format!(
            "{s} is a theme color: write it in braces, {{{s}}}, not in quotes"
        ));
    }
    let hex = s.strip_prefix('#').ok_or("expected a color like #rrggbb")?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("{s} is not a color"));
    }
    let d = |i: usize, n: usize| {
        u8::from_str_radix(&hex[i..i + n], 16).map_err(|_| format!("{s} is not a color"))
    };
    let dd = |i: usize| d(i, 1).map(|v| v * 17);
    match hex.len() {
        3 => Ok(Rgba {
            r: dd(0)?,
            g: dd(1)?,
            b: dd(2)?,
            a: 255,
        }),
        4 => Ok(Rgba {
            r: dd(0)?,
            g: dd(1)?,
            b: dd(2)?,
            a: dd(3)?,
        }),
        6 => Ok(Rgba {
            r: d(0, 2)?,
            g: d(2, 2)?,
            b: d(4, 2)?,
            a: 255,
        }),
        8 => Ok(Rgba {
            r: d(0, 2)?,
            g: d(2, 2)?,
            b: d(4, 2)?,
            a: d(6, 2)?,
        }),
        _ => Err(format!("{s} is not a color")),
    }
}

/// `c` written as `#rrggbbaa`.
fn hex(c: Rgba) -> String {
    format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a)
}

fn fill(v: &Value) -> Result<Fill, String> {
    match v {
        Value::Str(s, _) => fill_of(s),
        // Each theme's string differs only in its colors, so the two fills
        // have the same shape.
        Value::Themed { light, dark, .. } => match (fill_of(light)?, fill_of(dark)?) {
            (Fill::Solid(l), Fill::Solid(d)) => Ok(Fill::Solid(Color {
                light: l.light,
                dark: d.light,
            })),
            (
                Fill::Linear { angle, stops: l },
                Fill::Linear {
                    stops: d,
                    angle: dark_angle,
                },
            ) if angle == dark_angle && l.len() == d.len() => Ok(Fill::Linear {
                angle,
                stops: l
                    .iter()
                    .zip(&d)
                    .map(|((l, at), (d, _))| {
                        (
                            Color {
                                light: l.light,
                                dark: d.light,
                            },
                            *at,
                        )
                    })
                    .collect(),
            }),
            _ => Err(format!("{} is not the same fill in both themes", show(v))),
        },
        _ => Err(format!("expected a color or a gradient, found {}", show(v))),
    }
}

/// A fill written as a string, its colors the same in both themes.
fn fill_of(s: &str) -> Result<Fill, String> {
    let Some(inner) = s
        .strip_prefix("linear-gradient(")
        .and_then(|r| r.strip_suffix(')'))
    else {
        return rgba(s).map(|c| Fill::Solid(Color::fixed(c)));
    };
    let wrong =
        || format!("expected \"linear-gradient(<angle>deg, <color> <stop>%, …)\", found \"{s}\"");
    let mut parts = inner.split(',').map(str::trim);
    let angle = parts
        .next()
        .and_then(|a| a.strip_suffix("deg"))
        .ok_or_else(wrong)?;
    let angle = parse::number(angle.trim())? as f32;
    let mut stops = Vec::new();
    for part in parts {
        let (c, at) = part.split_once(' ').ok_or_else(wrong)?;
        let at = at.trim().strip_suffix('%').ok_or_else(wrong)?;
        let at = parse::number(at)? as f32;
        stops.push((Color::fixed(rgba(c)?), at / 100.0));
    }
    if stops.len() < 2 {
        return Err(wrong());
    }
    Ok(Fill::Linear { angle, stops })
}

/// Whether a value holds a theme color, at any depth.
fn has_theme(v: &Value) -> bool {
    match v {
        Value::Themed { .. } | Value::Theme(..) => true,
        Value::Array(items) => items.iter().any(has_theme),
        Value::Object(fields) => fields.iter().any(|(_, v)| has_theme(v)),
        _ => false,
    }
}

fn shadow(v: &Value) -> Result<Shadow, String> {
    let Value::Object(fields) = v else {
        return Err("expected { x, y, blur, spread, color }".into());
    };
    let mut s = Shadow {
        x: 0.0,
        y: 0.0,
        blur: 0.0,
        spread: 0.0,
        color: Color::fixed(Rgba {
            r: 0,
            g: 0,
            b: 0,
            a: 64,
        }),
        at: Origin::default(),
    };
    for (k, v) in fields {
        match k.as_str() {
            "x" => s.x = num(v)?,
            "y" => s.y = num(v)?,
            "blur" => s.blur = num(v)?,
            "spread" => s.spread = num(v)?,
            "color" => s.color = color(v)?,
            other => return Err(format!("a shadow has no {other}")),
        }
    }
    Ok(s)
}

mod again;

pub use again::{Outcome, Resolution};

#[cfg(test)]
mod tests;
