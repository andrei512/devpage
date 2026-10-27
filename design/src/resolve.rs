//! Syntax tree → scene: imports and props substituted, every attribute
//! checked, typed and defaulted.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::Arc;

use vello_cpu::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};

use crate::Problem;
use crate::parse::{self, Child, Element, Pos, TextPart, Value};

/// One color's value: straight-alpha RGBA.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// Which of a design's two themes it is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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
    pub children: Vec<Node>,
}

impl Node {
    fn new(kind: Kind, name: &str) -> Node {
        Node {
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
            children: Vec::new(),
        }
    }
}

/// Reads a design's files by their path from the top of the served folder.
pub trait Files {
    /// A file's bytes: a design's text, or an `Image`'s file.
    fn bytes(&self, path: &str) -> Option<Vec<u8>>;
    /// A design file's text.
    fn read(&self, path: &str) -> Option<String> {
        String::from_utf8(self.bytes(path)?).ok()
    }
    /// The image file at `path`, decoded, or why it doesn't decode; `None`
    /// when there is no such file. A store that keeps files between resolves
    /// keeps their decoded images too, so a save doesn't decode them again.
    fn image(&self, path: &str) -> Option<Result<Arc<ImageFile>, String>> {
        self.bytes(path)
            .map(|bytes| ImageFile::new(bytes).map(Arc::new))
    }
}

impl Files for HashMap<String, String> {
    fn bytes(&self, path: &str) -> Option<Vec<u8>> {
        self.get(path).map(|s| s.clone().into_bytes())
    }
}

/// A resolved design: its tree, each distinct image file its `Image`s show,
/// the files it was made from, and whether it is **themed** — whether any
/// `theme.` reference reached its nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub root: Node,
    pub images: ImageFiles,
    pub files: Vec<String>,
    pub themed: bool,
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
    let mut r = Resolver {
        files,
        problems: Vec::new(),
        read: Vec::new(),
        stack: Vec::new(),
        missing: Vec::new(),
        depth: 0,
        images: ImageFiles::new(),
        by_path: HashMap::new(),
        themes: HashMap::new(),
        themed: false,
    };
    let root = r.design(path, &[], None, "", None);
    if r.problems.is_empty() {
        Ok(Scene {
            root: root.expect("no problems means a root"),
            images: r.images,
            files: r.read,
            themed: r.themed,
        })
    } else {
        Err(r.problems)
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
    /// The themes each folder's designs use, merged from every `themes.yml`
    /// from that folder up, by folder: read once per resolve.
    themes: HashMap<String, Rc<Themes>>,
    /// Whether a `theme.` reference reached a node.
    themed: bool,
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
}

/// One `themes.yml`'s two themes, as far as they are valid.
struct ThemesFile {
    light: BTreeMap<String, Rgba>,
    dark: BTreeMap<String, Rgba>,
}

/// The file a folder's themes are in.
pub const THEMES_FILE: &str = "themes.yml";

/// How deep a resolved design's tree may nest, counting through instances:
/// the parser's own limit for one file, and so far beyond any real design.
const MAX_DEPTH: usize = parse::MAX_DEPTH;

type Props = HashMap<String, Value>;

impl Resolver<'_> {
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
        for attr in &mut el.attrs {
            self.bind(path, &mut attr.value);
        }
        for child in &mut el.children {
            if let Child::Element(e) = child {
                self.bind_element(path, e);
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
                other => {
                    let pos = key_pos(&text, None, other).unwrap_or(start);
                    self.problem(
                        path,
                        pos,
                        format!("{other}: a themes file has kind, light and dark only"),
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

    /// The root node of the design at `path`, with `args` given to its props.
    fn design(
        &mut self,
        path: &str,
        args: &[parse::Attr],
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
        let mut file = match parse::parse(&source) {
            Ok(file) => file,
            Err(mut problem) => {
                problem.file = path.to_string();
                self.problems.push(problem);
                return None;
            }
        };
        // Bound now, before any value leaves this file as a prop: a theme
        // color is this file's, wherever it is used.
        self.bind_element(path, &mut file.root);
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
                        props.insert(k.clone(), v.clone());
                    }
                }
                _ => self.problem(
                    path,
                    attr.pos,
                    "props: expected an object of defaults".into(),
                ),
            }
        }
        let (caller, caller_pos) = from.unwrap_or((path, file.root.pos));
        let caller = caller.to_string();
        let mut overrides = Vec::new();
        for arg in args {
            if matches!(
                arg.name.as_str(),
                "x" | "y" | "width" | "height" | "name" | "opacity" | "onClick"
            ) {
                overrides.push(arg.clone());
            } else if props.contains_key(&arg.name) {
                props.insert(arg.name.clone(), arg.value.clone());
            } else {
                self.problem(
                    &caller,
                    arg.pos,
                    format!("{used_as} has no prop \"{}\"", arg.name),
                );
            }
        }
        let _ = caller_pos;
        self.stack.push(path.to_string());
        let mut root = self.element(path, &file.root, &props, &imports, true, parent);
        self.stack.pop();
        if let Some(root) = root.as_mut() {
            for attr in &overrides {
                // The caller's own props are already substituted into these.
                self.common(&caller, root, attr, attr.value.clone());
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
            let args: Vec<parse::Attr> = attrs
                .iter()
                .map(|a| parse::Attr {
                    name: a.name.clone(),
                    value: substitute(&a.value, props),
                    pos: a.pos,
                })
                .collect();
            let target = target.clone();
            return self.design(&target, &args, Some((path, el.pos)), &el.name, parent);
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
        for attr in attrs {
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
            let value = match &attr.value {
                Value::Prop(p) => match props.get(p) {
                    Some(v) => v.clone(),
                    None => {
                        self.problem(path, attr.pos, format!("{}: no prop \"{p}\"", attr.name));
                        continue;
                    }
                },
                v => v.clone(),
            };
            self.themed |= has_theme(&value);
            self.fill_needs_layout(path, attr, &value, parent);
            self.place_needs_no_layout(path, attr, parent);
            self.attribute(path, &mut node, attr, value);
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
                                TextPart::Prop(p) => match props.get(p) {
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
            node.text.get_or_insert_with(default_text).content = content;
        } else {
            for child in &el.children {
                match child {
                    Child::Element(e) => {
                        if kind != Kind::Frame {
                            self.problem(path, e.pos, format!("{} takes no children", el.name));
                        } else if let Some(n) =
                            self.element(path, e, props, imports, false, Some(node.layout))
                        {
                            node.children.push(n);
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
    fn attribute(&mut self, path: &str, node: &mut Node, attr: &parse::Attr, value: Value) {
        let kind = node.kind;
        let ok = match (attr.name.as_str(), kind) {
            ("x" | "y" | "width" | "height" | "name" | "opacity" | "onClick", _) => {
                self.common(path, node, attr, value);
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
                Ok(w) if [400.0, 500.0, 600.0, 700.0].contains(&w) => {
                    Ok(node.text.get_or_insert_with(default_text).weight = w as u16)
                }
                _ => Err("expected 400, 500, 600 or 700".into()),
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
                    vello_cpu::kurbo::BezPath::from_svg(&d)
                        .map_err(|e| format!("not SVG path data: {e}"))
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
                Some(src) => match join(path, &src) {
                    None => Err(format!("{src} — outside the served folder")),
                    Some(at) => match self.image_file(&at) {
                        None => Err(format!("{src} — no such file")),
                        Some(Err(e)) => Err(format!("{src} {e}")),
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
            Some(at) if self.files.bytes(&at).is_none() => Err(format!("{action} — no such file")),
            Some(at) => Ok(format!("{verb} {at}")),
        }
    }

    fn common(&mut self, path: &str, node: &mut Node, attr: &parse::Attr, value: Value) {
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
                Some(action) => self
                    .interaction(path, &action)
                    .map(|a| node.on_click = Some(a)),
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
    use vello_cpu::kurbo::Shape;
    let b = vello_cpu::kurbo::BezPath::from_svg(d)
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

fn substitute(value: &Value, props: &Props) -> Value {
    match value {
        Value::Prop(p) => props.get(p).cloned().unwrap_or_else(|| value.clone()),
        v => v.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn files(list: &[(&str, &str)]) -> HashMap<String, String> {
        list.iter()
            .map(|(p, s)| (p.to_string(), s.to_string()))
            .collect()
    }

    #[test]
    fn an_instance_takes_its_props() {
        let fs = files(&[
            (
                "a/button.design",
                r##"<Frame props={{ label: "Button", fill: "#2563eb" }} fill={fill}><Text>{label}</Text></Frame>"##,
            ),
            (
                "a/screen.design",
                r#"import Button from "./button.design"
<Frame width={390}><Button label="Pay" x={10} opacity={0.5} /></Frame>"#,
            ),
        ]);
        let scene = resolve(&fs, "a/screen.design").unwrap();
        let button = &scene.root.children[0];
        assert_eq!(button.x, 10.0);
        assert_eq!(button.opacity, 0.5);
        assert_eq!(
            button.fill,
            Some(Fill::Solid(Color::fixed(Rgba {
                r: 0x25,
                g: 0x63,
                b: 0xeb,
                a: 255
            })))
        );
        assert_eq!(button.children[0].text.as_ref().unwrap().content, "Pay");
        assert_eq!(scene.files, vec!["a/screen.design", "a/button.design"]);
    }

    #[test]
    fn opacity_is_between_zero_and_one() {
        let fs = files(&[
            ("dot.design", "<Frame />"),
            (
                "screen.design",
                "import Dot from \"./dot.design\"\n<Frame opacity={1}>\n  <Rect opacity={50} />\n  <Dot opacity={-0.1} />\n</Frame>",
            ),
        ]);
        let problems: Vec<String> = resolve(&fs, "screen.design")
            .unwrap_err()
            .iter()
            .map(|p| p.to_string())
            .collect();
        assert_eq!(
            problems,
            vec![
                "screen.design:3:9: opacity: expected a number from 0 to 1, found 50",
                "screen.design:4:8: opacity: expected a number from 0 to 1, found -0.1",
            ]
        );
    }

    fn problems_in(fs: &HashMap<String, String>, path: &str) -> Vec<String> {
        resolve(fs, path)
            .unwrap_err()
            .iter()
            .map(|p| p.to_string())
            .collect()
    }

    #[test]
    fn a_color_with_any_character_but_hex_digits_is_a_problem() {
        // A pasted zero-width space, other non-ASCII, and plain non-hex.
        for (value, col) in [
            ("#fff\u{200b}", 13),
            ("#ffé", 13),
            ("#ff€f00", 13),
            ("#ggg", 13),
        ] {
            let fs = files(&[("a.design", &format!(r#"<Frame fill="{value}" />"#))]);
            assert_eq!(
                problems_in(&fs, "a.design"),
                vec![format!("a.design:1:8: fill: {value} is not a color")],
                "{value:?} at {col}"
            );
        }
    }

    #[test]
    fn props_belong_to_the_root_only() {
        let fs = files(&[(
            "a.design",
            r#"<Frame props={{ a: "x" }}>
  <Frame props={{ b: "y" }} />
</Frame>"#,
        )]);
        assert_eq!(
            problems_in(&fs, "a.design"),
            vec!["a.design:2:10: props: only a design's root declares props"]
        );
    }

    #[test]
    fn a_root_has_no_place_of_its_own() {
        let fs = files(&[
            (
                "dot.design",
                r#"<Frame x={40} y={30} width={10} height={10} />"#,
            ),
            (
                "a.design",
                r#"import Dot from "./dot.design"
<Frame width={100} height={100}>
  <Dot x={5} y={6} />
</Frame>"#,
            ),
        ]);
        let expected = [
            "dot.design:1:8: x: a design's root has no place of its own; place an instance with x and y where it is used",
            "dot.design:1:15: y: a design's root has no place of its own; place an instance with x and y where it is used",
        ];
        assert_eq!(problems_in(&fs, "dot.design"), expected);
        // Used as an instance, the root's own x and y are still a problem; the
        // instance's x and y are where it goes.
        assert_eq!(problems_in(&fs, "a.design"), expected);
        let fs = files(&[
            ("dot.design", r#"<Frame width={10} height={10} />"#),
            (
                "a.design",
                r#"import Dot from "./dot.design"
<Frame width={100} height={100}>
  <Dot x={5} y={6} />
</Frame>"#,
            ),
        ]);
        let dot = &resolve(&fs, "a.design").unwrap().root.children[0];
        assert_eq!((dot.x, dot.y), (5.0, 6.0));
    }

    #[test]
    fn fill_needs_a_parent_with_auto_layout() {
        let fs = files(&[
            ("dot.design", r#"<Frame width="fill" />"#),
            (
                "a.design",
                r#"import Dot from "./dot.design"
<Frame width={100} height={100}>
  <Rect width="fill" height={10} />
  <Dot />
  <Dot height="fill" />
  <Frame layout="row"><Rect width="fill" /></Frame>
</Frame>"#,
            ),
        ]);
        assert_eq!(
            problems_in(&fs, "a.design"),
            vec![
                "a.design:3:9: width: \"fill\" needs a parent with layout=\"row\" or \"column\"",
                "dot.design:1:8: width: \"fill\" needs a parent with layout=\"row\" or \"column\"",
                "dot.design:1:8: width: \"fill\" needs a parent with layout=\"row\" or \"column\"",
                "a.design:5:8: height: \"fill\" needs a parent with layout=\"row\" or \"column\"",
            ]
        );
    }

    #[test]
    fn a_tree_nests_at_most_so_deep_through_instances() {
        // Each file nests a little; through instances the tree goes too deep.
        let mut list = Vec::new();
        let depth = 40;
        for i in 0..depth {
            let body = if i + 1 < depth {
                let inner = "<Frame>".repeat(10) + "<N />" + &"</Frame>".repeat(10);
                format!("import N from \"./{}.design\"\n{inner}", i + 1)
            } else {
                "<Frame />".to_string()
            };
            list.push((format!("{i}.design"), body));
        }
        let fs: HashMap<String, String> = list.into_iter().collect();
        let found = problems_in(&fs, "0.design");
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].ends_with(&format!("nested more than {MAX_DEPTH} deep")),
            "{found:?}"
        );
    }

    #[test]
    fn every_problem_is_reported() {
        let fs = files(&[
            (
                "button.design",
                r#"<Frame props={{ label: "B" }} padding="12px"><Text>{label}</Text></Frame>"#,
            ),
            (
                "screen.design",
                "import Button from \"./button.design\"\nimport Row from \"./row.design\"\n<Frame>\n  <Button lable=\"x\" />\n  <Row />\n  <Blob />\n</Frame>",
            ),
        ]);
        let problems: Vec<String> = resolve(&fs, "screen.design")
            .unwrap_err()
            .iter()
            .map(|p| p.to_string())
            .collect();
        assert_eq!(
            problems,
            vec![
                "screen.design:2:1: import ./row.design — no such file",
                "screen.design:4:11: Button has no prop \"lable\"",
                "button.design:1:31: padding: expected pixels or an array of 2 or 4, found \"12px\"",
                "screen.design:6:3: unknown element Blob",
            ]
        );
    }

    #[test]
    fn an_import_of_a_missing_file_is_a_problem_even_unused() {
        let fs = files(&[("cart.design", "import Row from \"./row.design\"\n<Frame />")]);
        let problems: Vec<String> = resolve(&fs, "cart.design")
            .unwrap_err()
            .iter()
            .map(|p| p.to_string())
            .collect();
        assert_eq!(
            problems,
            vec!["cart.design:1:1: import ./row.design — no such file"]
        );
    }

    #[test]
    fn a_design_that_uses_itself() {
        let fs = files(&[(
            "a.design",
            "import A from \"./a.design\"\n<Frame><A /></Frame>",
        )]);
        let problems = resolve(&fs, "a.design").unwrap_err();
        assert_eq!(
            problems[0].to_string(),
            "a.design:2:8: a.design uses itself"
        );
    }

    /// Files as bytes, so a test can hold an image.
    struct Mem(HashMap<String, Vec<u8>>);

    impl Files for Mem {
        fn bytes(&self, path: &str) -> Option<Vec<u8>> {
            self.0.get(path).cloned()
        }
    }

    /// A 1×1 PNG that decodes.
    fn png() -> Vec<u8> {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, 1, 1);
        e.set_color(png::ColorType::Rgba);
        let mut w = e.write_header().unwrap();
        w.write_image_data(&[0, 0, 0, 255]).unwrap();
        drop(w);
        out
    }

    fn problems(source: &str) -> Vec<String> {
        let fs = Mem([
            ("d/a.design".to_string(), source.as_bytes().to_vec()),
            ("d/photo.png".to_string(), png()),
            ("d/notes.txt".to_string(), b"hi".to_vec()),
            (
                "d/b.design".to_string(),
                br#"<Frame props={{ label: "B" }}><Text>{label}</Text></Frame>"#.to_vec(),
            ),
        ]
        .into());
        match resolve(&fs, "d/a.design") {
            Ok(_) => Vec::new(),
            Err(p) => p.iter().map(|p| p.to_string()).collect(),
        }
    }

    #[test]
    fn each_problem_names_its_place() {
        let cases = [
            (
                "<Rect />",
                "d/a.design:1:1: the root element must be a Frame, not Rect",
            ),
            (
                "<Frame props=\"x\" />",
                "d/a.design:1:8: props: expected an object of defaults",
            ),
            (
                "<Frame fill=\"red\" />",
                "d/a.design:1:8: fill: expected a color like #rrggbb",
            ),
            (
                "<Frame fill=\"#12\" />",
                "d/a.design:1:8: fill: #12 is not a color",
            ),
            (
                "<Frame fill=\"linear-gradient(90deg, #fff 0%)\" />",
                "d/a.design:1:8: fill: expected \"linear-gradient(<angle>deg, <color> <stop>%, …)\", found \"linear-gradient(90deg, #fff 0%)\"",
            ),
            (
                "<Frame width=\"wide\" />",
                "d/a.design:1:8: width: expected pixels, \"hug\" or \"fill\", found \"wide\"",
            ),
            (
                "<Frame gap=\"4\" />",
                "d/a.design:1:8: gap: expected a number, found \"4\"",
            ),
            (
                "<Frame radius={[1, 2]} />",
                "d/a.design:1:8: radius: expected pixels or an array of 4",
            ),
            (
                "<Frame layout=\"grid\" />",
                "d/a.design:1:8: layout: expected \"row\", \"column\" or \"none\"",
            ),
            (
                "<Frame align=\"space-between\" />",
                "d/a.design:1:8: align: expected \"start\", \"center\" or \"end\"",
            ),
            (
                "<Frame clip=\"yes\" />",
                "d/a.design:1:8: clip: expected true or false",
            ),
            (
                "<Frame shadow={{ blurr: 4 }} />",
                "d/a.design:1:8: shadow: a shadow has no blurr",
            ),
            (
                "<Frame onClick=\"go ./b.design\" />",
                "d/a.design:1:8: onClick: expected \"navigate <path>\", \"back\", \"overlay <path>\" or \"close\"",
            ),
            (
                "<Frame size={4} />",
                "d/a.design:1:8: Frame takes no attribute size",
            ),
            (
                "<Frame><Text fill=\"#fff\">a</Text></Frame>",
                "d/a.design:1:14: Text takes no attribute fill",
            ),
            (
                "<Frame><Text weight={450}>a</Text></Frame>",
                "d/a.design:1:14: weight: expected 400, 500, 600 or 700",
            ),
            (
                "<Frame><Text textAlign=\"justify\">a</Text></Frame>",
                "d/a.design:1:14: textAlign: expected \"left\", \"center\" or \"right\"",
            ),
            (
                "<Frame><Text><Rect /></Text></Frame>",
                "d/a.design:1:14: Text holds text only",
            ),
            (
                "<Frame>hello</Frame>",
                "d/a.design:1:8: text belongs in a Text, not a Frame",
            ),
            (
                "<Frame><Rect><Rect /></Rect></Frame>",
                "d/a.design:1:14: Rect takes no children",
            ),
            (
                "<Frame><Text>{nope}</Text></Frame>",
                "d/a.design:1:14: no text prop \"nope\"",
            ),
            (
                "<Frame gap={nope} />",
                "d/a.design:1:8: gap: no prop \"nope\"",
            ),
            (
                "<Frame><Vector viewBox=\"0 0 24 24\" /></Frame>",
                "d/a.design:1:8: Vector needs a path, d=\"M…\"",
            ),
            (
                "<Frame><Vector d=\"M0 0 Q\" /></Frame>",
                "d/a.design:1:16: d: not SVG path data: Unexpected EOF",
            ),
            (
                "<Frame><Vector d=\"M0 0L1 1\" viewBox=\"0 0 24\" /></Frame>",
                "d/a.design:1:29: viewBox: expected \"minX minY width height\"",
            ),
            (
                "<Frame><Image /></Frame>",
                "d/a.design:1:8: Image needs a file, src=\"…\"",
            ),
            (
                "<Frame><Image src=\"./gone.png\" /></Frame>",
                "d/a.design:1:15: src: ./gone.png — no such file",
            ),
            (
                "<Frame><Image src=\"../../x.png\" /></Frame>",
                "d/a.design:1:15: src: ../../x.png — outside the served folder",
            ),
            (
                "<Frame><Image src=\"./notes.txt\" /></Frame>",
                "d/a.design:1:15: src: ./notes.txt is not a PNG or JPEG file",
            ),
            (
                "<Frame><Rect clip /></Frame>",
                "d/a.design:1:14: Rect takes no attribute clip",
            ),
            (
                "<Frame fill=\"linear-gradient(2000000deg, #fff 0%, #000 100%)\" />",
                "d/a.design:1:8: fill: `2000000` is too large: numbers go up to 1000000",
            ),
            (
                "<Frame fill=\"linear-gradient(90deg, #fff -2000000%, #000 100%)\" />",
                "d/a.design:1:8: fill: `-2000000` is too small: numbers go down to -1000000",
            ),
            (
                "<Frame fill=\"linear-gradient(NaNdeg, #fff 0%, #000 100%)\" />",
                "d/a.design:1:8: fill: `NaN` is not a number",
            ),
            (
                "<Frame fill=\"linear-gradient(90deg, #fff 0%, #000 inf%)\" />",
                "d/a.design:1:8: fill: `inf` is not a number",
            ),
            (
                "<Frame><Vector d=\"M0 0L1 1\" viewBox=\"0 0 2000000 24\" /></Frame>",
                "d/a.design:1:29: viewBox: `2000000` is too large: numbers go up to 1000000",
            ),
            (
                "<Frame><Vector d=\"M0 0L1 1\" viewBox=\"0 0 inf 24\" /></Frame>",
                "d/a.design:1:29: viewBox: `inf` is not a number",
            ),
            (
                "<Frame><Vector d=\"M0 0L2000000 0\" /></Frame>",
                "d/a.design:1:16: d: `2000000` is too large: numbers go up to 1000000",
            ),
            (
                "<Frame><Vector d=\"M0 0L1e999 0\" /></Frame>",
                "d/a.design:1:16: d: `1e999` is too large: numbers go up to 1000000",
            ),
            (
                "<Frame><Vector d=\"M0 0L-3e6 0\" /></Frame>",
                "d/a.design:1:16: d: `-3e6` is too small: numbers go down to -1000000",
            ),
            (
                "import X from \"../../x.design\"\n<Frame />",
                "d/a.design:1:1: import ../../x.design — outside the served folder",
            ),
            (
                "import B from \"./b.design\"\n<Frame><B fill=\"#fff\" /></Frame>",
                "d/a.design:2:11: B has no prop \"fill\"",
            ),
            (
                "import B from \"./b.design\"\n<Frame><B>x</B></Frame>",
                "d/a.design:2:8: B takes no children",
            ),
            (
                "<Frame><Blob /></Frame>",
                "d/a.design:1:8: unknown element Blob",
            ),
        ];
        for (source, expected) in cases {
            assert_eq!(problems(source), vec![expected.to_string()], "{source}");
        }
    }

    /// An image file is decoded whole while resolving, so a broken one is a
    /// problem at its `src` — not a blank box — wherever the design is shown.
    #[test]
    fn an_image_that_does_not_decode_is_a_problem() {
        let mut broken = b"\x89PNG\r\n\x1a\n".to_vec();
        broken.extend(b"not a PNG past its signature");
        let shirt = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../docs/designs/checkout/shirt.jpg"
        ))
        .unwrap();
        let truncated = shirt[..shirt.len() / 2].to_vec();
        for (name, data, expected) in [
            (
                "broken.png",
                broken,
                "d/a.design:2:8: src: ./broken.png is a PNG that doesn't decode: ",
            ),
            (
                "shirt.jpg",
                truncated,
                "d/a.design:2:8: src: ./shirt.jpg is a JPEG that doesn't decode: ",
            ),
        ] {
            let source =
                format!("<Frame>\n<Image src=\"./{name}\" width={{50}} height={{50}} />\n</Frame>");
            let fs = Mem([
                ("d/a.design".to_string(), source.into_bytes()),
                (format!("d/{name}"), data),
            ]
            .into());
            let found: Vec<String> = resolve(&fs, "d/a.design")
                .unwrap_err()
                .iter()
                .map(|p| p.to_string())
                .collect();
            assert_eq!(found.len(), 1, "{found:?}");
            assert!(found[0].starts_with(expected), "{found:?}");
        }
        // The whole file decodes: its pixels go with the scene.
        let fs = Mem([
            (
                "a.design".to_string(),
                br#"<Frame><Image src="./s.jpg" /></Frame>"#.to_vec(),
            ),
            ("s.jpg".to_string(), shirt),
        ]
        .into());
        let scene = resolve(&fs, "a.design").unwrap();
        let image = scene.root.children[0].image.as_ref().unwrap();
        let file = &scene.images[&image.id];
        assert_eq!((file.width, file.height), (160, 160));
        assert_eq!((file.pixels_width, file.pixels_height), (160, 160));
    }

    /// A file larger than [`MAX_IMAGE_SIDE`] is drawn from its pixels
    /// resampled to fit it, keeping its aspect; an `Image` still hugs the
    /// file's own size.
    #[test]
    fn a_large_image_file_is_resampled_once_to_fit() {
        let mut data = Vec::new();
        let mut e = png::Encoder::new(&mut data, 9000, 10);
        e.set_color(png::ColorType::Rgba);
        let mut w = e.write_header().unwrap();
        // Red to x = 4500, blue past it.
        let row: Vec<u8> = (0..9000)
            .flat_map(|x| {
                if x <= 4500 {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 255, 255]
                }
            })
            .collect();
        w.write_image_data(&row.repeat(10)).unwrap();
        drop(w);
        let file = ImageFile::new(data).unwrap();
        assert_eq!((file.width, file.height), (9000, 10));
        assert_eq!((file.pixels_width, file.pixels_height), (4096, 5));
        let px = |x: usize| &file.pixels[4 * x..][..4];
        assert_eq!(px(0), [255, 0, 0, 255]);
        assert_eq!(px(4095), [0, 0, 255, 255]);
        // Column 2048 covers x = 4500 to 4502.2: one part red, 1.2 blue.
        assert_eq!(px(2048), [116, 0, 139, 255]);
        // A fitting file is drawn from its own pixels.
        let (same, w, h) = fit(&[1, 2, 3, 4], 1, 1, MAX_IMAGE_SIDE);
        assert_eq!((same, w, h), (vec![1, 2, 3, 4], 1, 1));
        // A transparent pixel lends no color to its neighbour.
        let (mixed, _, _) = fit(&[255, 255, 255, 0, 0, 0, 255, 255], 2, 1, 1);
        assert_eq!(mixed, [0, 0, 255, 128]);
    }

    #[test]
    fn an_interaction_is_relative_to_the_file_that_wrote_it() {
        let fs = files(&[
            (
                "app/a.design",
                "import B from \"../lib/b.design\"\n<Frame><B /><B onClick=\"overlay ./c.design\" /></Frame>",
            ),
            ("app/c.design", "<Frame />"),
            (
                "lib/b.design",
                "<Frame onClick=\"navigate ./next.design\" />",
            ),
            ("lib/next.design", "<Frame />"),
        ]);
        let root = resolve(&fs, "app/a.design").unwrap().root;
        assert_eq!(
            root.children[0].on_click.as_deref(),
            Some("navigate lib/next.design")
        );
        assert_eq!(
            root.children[1].on_click.as_deref(),
            Some("overlay app/c.design")
        );
    }

    #[test]
    fn an_interaction_to_nowhere_is_a_problem() {
        for (click, expected) in [
            (
                "navigate ./nope.design",
                "d/a.design:1:8: onClick: navigate ./nope.design — no such file",
            ),
            (
                "overlay ../../x.design",
                "d/a.design:1:8: onClick: overlay ../../x.design — outside the served folder",
            ),
        ] {
            assert_eq!(
                problems(&format!("<Frame onClick=\"{click}\" />")),
                vec![expected.to_string()],
                "{click}"
            );
        }
    }

    #[test]
    fn a_valid_design_has_no_problems() {
        assert_eq!(
            problems(
                r##"import B from "./b.design"
<Frame fill="linear-gradient(90deg, #ffffff 0%, #000000ff 100%)">
  <B label="Go" x={4} y={5} width={80} height="hug" name="Go" onClick="back" />
  <Vector d="M0 0L24 24" viewBox="0 0 24 24" stroke="#000" />
  <Frame radius={8} clip><Image src="./photo.png" radius={[1, 2, 3, 4]} /></Frame>
</Frame>"##
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn attributes_take_their_defaults() {
        let fs = files(&[("a.design", "<Frame><Text>a</Text><Rect /></Frame>")]);
        let root = resolve(&fs, "a.design").unwrap().root;
        assert_eq!((root.width, root.height), (Size::Hug, Size::Hug));
        assert_eq!(
            (root.layout, root.gap, root.padding),
            (Layout::None, 0.0, [0.0; 4])
        );
        assert_eq!((root.align, root.justify), (Align::Start, Align::Start));
        assert_eq!(
            (root.opacity, root.fill.clone(), root.clip),
            (1.0, None, false)
        );
        let text = root.children[0].text.clone().unwrap();
        assert_eq!(
            text,
            TextStyle {
                content: "a".into(),
                ..default_text()
            }
        );
        assert_eq!(
            (text.size, text.weight, text.line_height, text.align),
            (14.0, 400, 1.4, TextAlign::Left)
        );
        assert_eq!(root.children[1].stroke_width, 1.0);
        assert_eq!(root.children[1].radius, [0.0; 4]);
    }

    #[test]
    fn values_parse_into_their_types() {
        let fs = Mem(
            [
                (
                    "a.design".to_string(),
                    br##"<Frame padding={[1, 2]} radius={[1, 2, 3, 4]} fill="linear-gradient(135deg, #f00 0%, #0000ff80 100%)">
  <Frame padding={[1, 2, 3, 4]} fill="#abcd" />
  <Vector d="M2 4L10 20" />
  <Image src="./p.png" />
</Frame>"##
                        .to_vec(),
                ),
                ("p.png".to_string(), png()),
            ]
            .into(),
        );
        let scene = resolve(&fs, "a.design").unwrap();
        let root = &scene.root;
        assert_eq!(root.padding, [1.0, 2.0, 1.0, 2.0]);
        assert_eq!(root.radius, [1.0, 2.0, 3.0, 4.0]);
        let c = |r, g, b, a| Color::fixed(Rgba { r, g, b, a });
        assert_eq!(
            root.fill,
            Some(Fill::Linear {
                angle: 135.0,
                stops: vec![(c(255, 0, 0, 255), 0.0), (c(0, 0, 255, 0x80), 1.0)]
            })
        );
        assert_eq!(root.children[0].padding, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(
            root.children[0].fill,
            Some(Fill::Solid(c(0xaa, 0xbb, 0xcc, 0xdd)))
        );
        // A vector with no view box is drawn from its path's own bounds.
        assert_eq!(
            root.children[1].vector.as_ref().unwrap().view_box,
            [2.0, 4.0, 8.0, 16.0]
        );
        assert_eq!(root.children[2].image.as_ref().unwrap().src, "p.png");
        assert_eq!(scene.files, vec!["a.design", "p.png"]);
    }

    #[test]
    fn paths_stay_in_the_served_folder() {
        assert_eq!(
            join("a/b.design", "./c.design").as_deref(),
            Some("a/c.design")
        );
        assert_eq!(
            join("a/b.design", "../c.design").as_deref(),
            Some("c.design")
        );
        assert_eq!(join("a/b.design", "../../c.design"), None);
        assert_eq!(
            join("/a/b.design", "x/../y.png").as_deref(),
            Some("a/y.png")
        );
    }

    #[test]
    fn an_interaction_leads_only_to_a_design() {
        for (click, expected) in [
            (
                "navigate ./notes.txt",
                "d/a.design:1:8: onClick: navigate ./notes.txt — a design is a .design file",
            ),
            (
                "overlay ./photo.png",
                "d/a.design:1:8: onClick: overlay ./photo.png — a design is a .design file",
            ),
        ] {
            assert_eq!(
                problems(&format!("<Frame onClick=\"{click}\" />")),
                vec![expected.to_string()],
                "{click}"
            );
        }
        assert!(problems(r#"<Frame onClick="navigate ./b.design" />"#).is_empty());
    }

    #[test]
    fn sizes_and_spacing_are_never_negative() {
        for (attr, element) in [
            ("gap={-4}", "Frame"),
            ("padding={-1}", "Frame"),
            ("padding={[4, -2]}", "Frame"),
            ("radius={-3}", "Rect"),
            ("radius={[1, 2, -3, 4]}", "Rect"),
            ("strokeWidth={-1}", "Rect"),
            ("size={-12}", "Text"),
            ("lineHeight={-1}", "Text"),
        ] {
            let found = problems(&format!("<Frame><{element} {attr} /></Frame>"));
            assert_eq!(found.len(), 1, "{attr}: {found:?}");
            assert!(found[0].contains("of 0 or more"), "{attr}: {found:?}");
        }
    }

    #[test]
    fn an_import_name_is_unique_and_not_an_element() {
        let found = problems(
            r#"import B from "./b.design"
import B from "./b.design"
<Frame />"#,
        );
        assert_eq!(found, ["d/a.design:2:1: import B — B is already imported"]);
        let found = problems(
            r#"import Frame from "./b.design"
<Frame />"#,
        );
        assert_eq!(
            found,
            ["d/a.design:1:1: import Frame — Frame is a built-in element"]
        );
    }

    #[test]
    fn justify_names_all_it_takes() {
        assert_eq!(
            problems(r#"<Frame justify="middle" />"#),
            [
                "d/a.design:1:8: justify: expected \"start\", \"center\", \"end\" or \"space-between\""
            ]
        );
        assert_eq!(
            problems(r#"<Frame align="middle" />"#),
            ["d/a.design:1:8: align: expected \"start\", \"center\" or \"end\""]
        );
    }

    #[test]
    fn values_that_would_be_ignored_are_problems() {
        // x and y place a child only in a frame with no layout.
        let found =
            problems(r#"<Frame layout="row"><Rect width={10} height={10} x={5} /></Frame>"#);
        assert!(
            found
                .iter()
                .any(|p| p.contains("x: places a child only in a frame with layout=\"none\"")),
            "{found:?}"
        );
        // The same on an instance placed in a row.
        let found = problems(
            r#"import B from "./b.design"
<Frame layout="row"><B y={3} /></Frame>"#,
        );
        assert!(
            found.iter().any(|p| p.contains("y: places a child only")),
            "{found:?}"
        );
        // Negative sizes.
        let found = problems(r#"<Frame width={-1} height={-2} />"#);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(
            found[0].contains("width: expected pixels of 0 or more"),
            "{found:?}"
        );
        // An attribute given twice.
        let found = problems(r##"<Frame fill="#ff0000" fill="#00ff00" />"##);
        assert!(
            found.iter().any(|p| p.contains("fill is given twice")),
            "{found:?}"
        );
        let found = problems(
            r#"import B from "./b.design"
<Frame><B label="a" label="b" /></Frame>"#,
        );
        assert!(
            found.iter().any(|p| p.contains("label is given twice")),
            "{found:?}"
        );
        // An import of something that isn't a design says so.
        let found = problems(
            r#"import P from "./photo.png"
<Frame />"#,
        );
        assert_eq!(
            found,
            ["d/a.design:1:1: import ./photo.png — a design is a .design file"]
        );
        // x and y in a frame with no layout are fine.
        assert!(problems(r#"<Frame><Rect width={1} height={1} x={5} y={5} /></Frame>"#).is_empty());
    }

    const THEMES: &str = r##"kind: themes
light:
  surface: "#ffffff"
  text: "#000"
dark:
  surface: "#0f172a"
  text: "#f1f5f9"
"##;

    fn rgb(r: u8, g: u8, b: u8) -> Rgba {
        Rgba { r, g, b, a: 255 }
    }

    #[test]
    fn a_theme_color_has_a_value_in_each_theme() {
        let fs = files(&[
            ("themes.yml", THEMES),
            (
                "a.design",
                r##"<Frame fill={theme.surface} stroke="#ff0000"
       shadow={{ y: 2, color: theme.text }}>
  <Text color={theme.text}>Hi</Text>
  <Rect width={1} height={1} fill="linear-gradient(90deg, theme.surface 0%, #ff0000 100%)" />
</Frame>"##,
            ),
        ]);
        let scene = resolve(&fs, "a.design").unwrap();
        assert!(scene.themed);
        let surface = Color {
            light: rgb(255, 255, 255),
            dark: rgb(0x0f, 0x17, 0x2a),
        };
        let text = Color {
            light: rgb(0, 0, 0),
            dark: rgb(0xf1, 0xf5, 0xf9),
        };
        let root = &scene.root;
        assert_eq!(root.fill, Some(Fill::Solid(surface)));
        // A # color is the same in both.
        assert_eq!(root.stroke, Some(Color::fixed(rgb(255, 0, 0))));
        assert_eq!(root.shadow.as_ref().unwrap().color, text);
        assert_eq!(root.children[0].text.as_ref().unwrap().color, text);
        assert_eq!(
            root.children[1].fill,
            Some(Fill::Linear {
                angle: 90.0,
                stops: vec![(surface, 0.0), (Color::fixed(rgb(255, 0, 0)), 1.0)]
            })
        );
        assert_eq!(scene.files, vec!["a.design", "themes.yml"]);
        // A design with no theme color isn't themed, and reads no themes.
        let plain = resolve(
            &files(&[("themes.yml", THEMES), ("b.design", "<Frame />")]),
            "b.design",
        )
        .unwrap();
        assert!(!plain.themed);
        assert_eq!(plain.files, vec!["b.design"]);
    }

    /// Files that record every path asked for.
    struct Asked(HashMap<String, String>, std::cell::RefCell<Vec<String>>);

    impl Files for Asked {
        fn bytes(&self, path: &str) -> Option<Vec<u8>> {
            self.1.borrow_mut().push(path.to_string());
            self.0.bytes(path)
        }
    }

    #[test]
    fn the_nearest_themes_override_those_above_name_by_name() {
        let fs = Asked(
            files(&[
                ("themes.yml", THEMES),
                (
                    "app/themes.yml",
                    "kind: themes\nlight:\n  text: \"#111111\"\ndark: {}\n",
                ),
                // Another tool's file: passed over.
                ("app/screens/themes.yml", "kind: tokens\nlight: 1\n"),
                (
                    "app/screens/a.design",
                    "<Frame fill={theme.surface}><Text color={theme.text}>Hi</Text></Frame>",
                ),
            ]),
            Default::default(),
        );
        let scene = resolve(&fs, "app/screens/a.design").unwrap();
        assert_eq!(
            scene.root.fill,
            Some(Fill::Solid(Color {
                light: rgb(255, 255, 255),
                dark: rgb(0x0f, 0x17, 0x2a)
            }))
        );
        // Light's text from app/, dark's from the top.
        assert_eq!(
            scene.root.children[0].text.as_ref().unwrap().color,
            Color {
                light: rgb(0x11, 0x11, 0x11),
                dark: rgb(0xf1, 0xf5, 0xf9)
            }
        );
        // Every candidate is asked for once, the missing and the passed-over
        // ones too, so writing any of them counts as a change.
        let mut asked = fs.1.borrow().clone();
        asked.retain(|p| p.ends_with("themes.yml"));
        assert_eq!(
            asked,
            vec!["app/screens/themes.yml", "app/themes.yml", "themes.yml"]
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_theme_color_belongs_to_the_file_that_wrote_it() {
        let dark_red =
            "kind: themes\nlight:\n  accent: \"#ff0000\"\ndark:\n  accent: \"#880000\"\n";
        let blue = "kind: themes\nlight:\n  accent: \"#0000ff\"\ndark:\n  accent: \"#000088\"\n";
        let fs = files(&[
            ("kit/themes.yml", dark_red),
            (
                "kit/button.design",
                "<Frame props={{ fill: theme.accent }} fill={fill} />",
            ),
            ("app/themes.yml", blue),
            (
                "app/screen.design",
                "import Button from \"../kit/button.design\"\n<Frame layout=\"row\">\n  <Button />\n  <Button fill={theme.accent} />\n</Frame>",
            ),
        ]);
        let scene = resolve(&fs, "app/screen.design").unwrap();
        let fill = |n: usize| scene.root.children[n].fill.clone();
        // The default is the kit's red; the color the screen passes, its blue.
        assert_eq!(
            fill(0),
            Some(Fill::Solid(Color {
                light: rgb(255, 0, 0),
                dark: rgb(0x88, 0, 0)
            }))
        );
        assert_eq!(
            fill(1),
            Some(Fill::Solid(Color {
                light: rgb(0, 0, 255),
                dark: rgb(0, 0, 0x88)
            }))
        );
        assert!(scene.themed);
    }

    #[test]
    fn a_theme_color_that_isnt_there_is_a_problem_at_the_reference() {
        let one_sided = "kind: themes\nlight:\n  surface: \"#fff\"\n  edge: \"#000\"\ndark:\n  surface: \"#000\"\n";
        let design = r##"<Frame fill={theme.srface}>
  <Rect width={1} height={1} fill={theme.edge} />
  <Rect width={1} height={1} fill="linear-gradient(90deg, #000 0%, theme.nope 100%)" />
  <Text>{theme.surface}</Text>
</Frame>"##;
        let fs = files(&[("themes.yml", one_sided), ("a.design", design)]);
        assert_eq!(
            problems_in(&fs, "a.design"),
            vec![
                "a.design:1:14: theme.srface: the themes have no srface; they have edge, surface",
                "a.design:2:36: theme.edge: only the light theme has edge; a name needs a color in both",
                "a.design:3:68: theme.nope: the themes have no nope; they have edge, surface",
                "a.design:4:10: theme.surface is a color, not text: give it to color={theme.surface}",
            ]
        );
        // With no themes file above the design at all.
        let fs = files(&[("a/a.design", "<Frame fill={theme.surface} />")]);
        assert_eq!(
            problems_in(&fs, "a/a.design"),
            vec![
                "a/a.design:1:14: theme.surface: no themes.yml with kind: themes in this design's folder or above it"
            ]
        );
    }

    #[test]
    fn a_malformed_themes_file_is_a_problem_at_its_line() {
        let broken = r##"kind: themes
# the light one
light:
  surface: "#ffff"
  text: "#gg0000"
  edge: #000
  9lives: "#000"
dark: "#000"
colour: red
"##;
        let fs = files(&[
            ("themes.yml", broken),
            ("a.design", "<Frame fill={theme.surface} />"),
        ]);
        assert_eq!(
            problems_in(&fs, "a.design"),
            vec![
                "themes.yml:5:3: light.text: #gg0000 is not a color",
                "themes.yml:6:3: light.edge: no color: write it in quotes, \"#rrggbb\", as YAML reads # as a comment",
                "themes.yml:7:3: light.9lives: a name holds letters, digits, _ and -, and starts with a letter",
                "themes.yml:8:1: dark: expected a map of names to colors, like surface: \"#ffffff\"",
                "themes.yml:9:1: colour: a themes file has kind, light and dark only",
                "a.design:1:14: theme.surface: only the light theme has surface; a name needs a color in both",
            ]
        );
        let fs = files(&[
            ("themes.yml", "kind: themes\nlight: {}\n"),
            ("a.design", "<Frame fill={theme.surface} />"),
        ]);
        assert_eq!(
            problems_in(&fs, "a.design")[0],
            "themes.yml:1:1: a themes file has a light and a dark theme; this one has no dark"
        );
        let fs = files(&[
            ("themes.yml", "kind: themes\nlight: [\n"),
            ("a.design", "<Frame fill={theme.surface} />"),
        ]);
        assert!(
            problems_in(&fs, "a.design")[0]
                .starts_with("themes.yml:3:1: this themes file isn't valid YAML: "),
            "{:?}",
            problems_in(&fs, "a.design")
        );
        // A file that doesn't say it is a themes file is someone else's.
        let fs = files(&[("themes.yml", "light: [\n"), ("a.design", "<Frame />")]);
        assert!(resolve(&fs, "a.design").is_ok());
    }

    #[test]
    fn theme_is_no_props_or_imports_name() {
        let fs = files(&[
            ("themes.yml", THEMES),
            ("b.design", "<Frame />"),
            (
                "a.design",
                "import theme from \"./b.design\"\n<Frame props={{ theme: \"#fff\" }} fill={theme} />",
            ),
        ]);
        assert_eq!(
            problems_in(&fs, "a.design"),
            vec![
                "a.design:1:1: import theme — theme is the name of the design's themes",
                "a.design:2:8: props: theme is the name of the design's themes, not a prop",
                "a.design:2:34: fill: no prop \"theme\"",
            ]
        );
        // In quotes it is a string, not a color.
        let fs = files(&[
            ("themes.yml", THEMES),
            ("a.design", "<Frame fill=\"theme.surface\" />"),
        ]);
        assert_eq!(
            problems_in(&fs, "a.design"),
            vec![
                "a.design:1:8: fill: theme.surface is a theme color: write it in braces, {theme.surface}, not in quotes"
            ]
        );
    }
}
