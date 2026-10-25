//! Syntax tree → scene: imports and props substituted, every attribute
//! checked, typed and defaulted.

use std::collections::HashMap;

use crate::Problem;
use crate::parse::{self, Child, Element, Pos, TextPart, Value};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
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

/// An `Image`'s file: its address in the served folder and its bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub src: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shadow {
    pub x: f32,
    pub y: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Color,
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
    /// The design an instance came from, on the instance's root.
    pub design: Option<String>,
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
            design: None,
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
}

impl Files for HashMap<String, String> {
    fn bytes(&self, path: &str) -> Option<Vec<u8>> {
        self.get(path).map(|s| s.clone().into_bytes())
    }
}

/// A resolved design and the files it was made from.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub root: Node,
    pub files: Vec<String>,
}

/// Resolves the design at `path`, reporting every problem found.
pub fn resolve(files: &dyn Files, path: &str) -> Result<Scene, Vec<Problem>> {
    let mut r = Resolver {
        files,
        problems: Vec::new(),
        read: Vec::new(),
        stack: Vec::new(),
    };
    let root = r.design(path, &[], None, "");
    if r.problems.is_empty() {
        Ok(Scene {
            root: root.expect("no problems means a root"),
            files: r.read,
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
}

type Props = HashMap<String, Value>;

impl Resolver<'_> {
    fn problem(&mut self, file: &str, pos: Pos, message: String) {
        self.problems.push(Problem {
            file: file.to_string(),
            pos,
            message,
        });
    }

    /// The root node of the design at `path`, with `args` given to its props.
    fn design(
        &mut self,
        path: &str,
        args: &[parse::Attr],
        from: Option<(&str, Pos)>,
        used_as: &str,
    ) -> Option<Node> {
        if self.stack.iter().any(|p| p == path) {
            let (file, pos) = from.unwrap_or((path, Pos { line: 1, col: 1 }));
            self.problem(file, pos, format!("{path} uses itself"));
            return None;
        }
        let Some(source) = self.files.read(path) else {
            let (file, pos) = from.unwrap_or((path, Pos { line: 1, col: 1 }));
            self.problem(file, pos, format!("{path} — no such file"));
            return None;
        };
        if !self.read.iter().any(|p| p == path) {
            self.read.push(path.to_string());
        }
        let file = match parse::parse(&source) {
            Ok(file) => file,
            Err(mut problem) => {
                problem.file = path.to_string();
                self.problems.push(problem);
                return None;
            }
        };
        let mut imports = HashMap::new();
        for import in &file.imports {
            match join(path, &import.path) {
                Some(target) => {
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
                "x" | "y" | "width" | "height" | "name" | "onClick"
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
        let mut root = self.element(path, &file.root, &props, &imports);
        self.stack.pop();
        if let Some(root) = root.as_mut() {
            root.design = Some(path.to_string());
            for attr in &overrides {
                // The caller's own props are already substituted into these.
                self.common(&caller, root, attr, attr.value.clone());
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
    ) -> Option<Node> {
        if let Some(target) = imports.get(&el.name) {
            if !el.children.is_empty() {
                self.problem(path, el.pos, format!("{} takes no children", el.name));
            }
            let args: Vec<parse::Attr> = el
                .attrs
                .iter()
                .map(|a| parse::Attr {
                    name: a.name.clone(),
                    value: substitute(&a.value, props),
                    pos: a.pos,
                })
                .collect();
            let target = target.clone();
            return self.design(&target, &args, Some((path, el.pos)), &el.name);
        }
        let kind = match el.name.as_str() {
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
        for attr in &el.attrs {
            if attr.name == "props" {
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
                                    Some(Value::Str(s)) => content.push_str(s),
                                    Some(Value::Num(n)) => content.push_str(&n.to_string()),
                                    _ => self.problem(path, *pos, format!("no text prop \"{p}\"")),
                                },
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
                        } else if let Some(n) = self.element(path, e, props, imports) {
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
            ("strokeWidth", k) if k != Kind::Text => num(&value).map(|n| node.stroke_width = n),
            ("radius", k) if k != Kind::Text => match &value {
                Value::Num(n) => Ok(node.radius = [*n as f32; 4]),
                Value::Array(a) if a.len() == 4 => {
                    nums(a).map(|v| node.radius = [v[0], v[1], v[2], v[3]])
                }
                _ => Err("expected pixels or an array of 4".into()),
            },
            ("shadow", k) if k != Kind::Text => shadow(&value).map(|s| node.shadow = Some(s)),
            ("clip", k) if k != Kind::Text => match value {
                Value::Bool(b) => Ok(node.clip = b),
                _ => Err("expected true or false".into()),
            },
            ("layout", Kind::Frame) => match str_of(&value).as_deref() {
                Some("row") => Ok(node.layout = Layout::Row),
                Some("column") => Ok(node.layout = Layout::Column),
                Some("none") => Ok(node.layout = Layout::None),
                _ => Err("expected \"row\", \"column\" or \"none\"".into()),
            },
            ("gap", Kind::Frame) => num(&value).map(|n| node.gap = n),
            ("padding", Kind::Frame) => match &value {
                Value::Num(n) => Ok(node.padding = [*n as f32; 4]),
                Value::Array(a) if a.len() == 2 => {
                    nums(a).map(|v| node.padding = [v[0], v[1], v[0], v[1]])
                }
                Value::Array(a) if a.len() == 4 => {
                    nums(a).map(|v| node.padding = [v[0], v[1], v[2], v[3]])
                }
                other => Err(format!(
                    "expected pixels or an array of 2 or 4, found {}",
                    show(other)
                )),
            },
            ("align", Kind::Frame) => align(&value, false).map(|a| node.align = a),
            ("justify", Kind::Frame) => align(&value, true).map(|a| node.justify = a),
            ("size", Kind::Text) => {
                num(&value).map(|n| node.text.get_or_insert_with(default_text).size = n)
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
                num(&value).map(|n| node.text.get_or_insert_with(default_text).line_height = n)
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
                Some(d) => match vello_cpu::kurbo::BezPath::from_svg(&d) {
                    Ok(_) => {
                        let view_box = node.vector.as_ref().map(|v| v.view_box);
                        let view_box = view_box.unwrap_or_else(|| bounds(&d));
                        Ok(node.vector = Some(Vector { d, view_box }))
                    }
                    Err(e) => Err(format!("not SVG path data: {e}")),
                },
                None => Err("expected SVG path data".into()),
            },
            ("viewBox", Kind::Vector) => match str_of(&value).as_deref().map(view_box) {
                Some(Some(b)) => {
                    let v = node.vector.get_or_insert_with(|| Vector {
                        d: String::new(),
                        view_box: b,
                    });
                    Ok(v.view_box = b)
                }
                _ => Err("expected \"minX minY width height\"".into()),
            },
            ("src", Kind::Image) => match str_of(&value) {
                Some(src) => match join(path, &src) {
                    None => Err(format!("{src} — outside the served folder")),
                    Some(at) => match self.files.bytes(&at) {
                        None => Err(format!("{src} — no such file")),
                        Some(data) if !is_image(&data) => {
                            Err(format!("{src} is not a PNG or JPEG file"))
                        }
                        Some(data) => {
                            if !self.read.iter().any(|p| p == &at) {
                                self.read.push(at.clone());
                            }
                            Ok(node.image = Some(Image { src: at, data }))
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

    fn common(&mut self, path: &str, node: &mut Node, attr: &parse::Attr, value: Value) {
        let ok = match attr.name.as_str() {
            "x" => num(&value).map(|n| node.x = n),
            "y" => num(&value).map(|n| node.y = n),
            "width" => size(&value).map(|s| node.width = s),
            "height" => size(&value).map(|s| node.height = s),
            "name" => str_of(&value)
                .map(|s| node.name = s)
                .ok_or_else(|| "expected a string".into()),
            "opacity" => num(&value).map(|n| node.opacity = n),
            "onClick" => str_of(&value)
                .filter(|s| interaction_ok(s))
                .map(|s| node.on_click = Some(s))
                .ok_or_else(|| {
                    "expected \"navigate <path>\", \"back\", \"overlay <path>\" or \"close\"".into()
                }),
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
        color: Color {
            r: 0x0f,
            g: 0x17,
            b: 0x2a,
            a: 255,
        },
        line_height: 1.4,
        align: TextAlign::Left,
    }
}

fn is_image(data: &[u8]) -> bool {
    data.starts_with(b"\x89PNG\r\n\x1a\n") || data.starts_with(&[0xff, 0xd8, 0xff])
}

fn view_box(s: &str) -> Option<[f32; 4]> {
    let v: Vec<f32> = s
        .split([' ', ','])
        .filter(|p| !p.is_empty())
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    match v[..] {
        [x, y, w, h] if w > 0.0 && h > 0.0 => Some([x, y, w, h]),
        _ => None,
    }
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
        Value::Str(s) => format!("\"{s}\""),
        Value::Num(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Array(_) => "an array".into(),
        Value::Object(_) => "an object".into(),
        Value::Prop(p) => p.clone(),
    }
}

fn str_of(v: &Value) -> Option<String> {
    match v {
        Value::Str(s) => Some(s.clone()),
        _ => None,
    }
}

fn num(v: &Value) -> Result<f32, String> {
    match v {
        Value::Num(n) => Ok(*n as f32),
        other => Err(format!("expected a number, found {}", show(other))),
    }
}

fn nums(a: &[Value]) -> Result<Vec<f32>, String> {
    a.iter().map(num).collect()
}

fn size(v: &Value) -> Result<Size, String> {
    match v {
        Value::Num(n) => Ok(Size::Px(*n as f32)),
        Value::Str(s) if s == "hug" => Ok(Size::Hug),
        Value::Str(s) if s == "fill" => Ok(Size::Fill),
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
        _ => Err("expected \"start\", \"center\" or \"end\"".into()),
    }
}

fn color(v: &Value) -> Result<Color, String> {
    let Value::Str(s) = v else {
        return Err(format!("expected a color, found {}", show(v)));
    };
    let hex = s.strip_prefix('#').ok_or("expected a color like #rrggbb")?;
    let d = |i: usize, n: usize| {
        u8::from_str_radix(&hex[i..i + n], 16).map_err(|_| format!("{s} is not a color"))
    };
    let dd = |i: usize| d(i, 1).map(|v| v * 17);
    match hex.len() {
        3 => Ok(Color {
            r: dd(0)?,
            g: dd(1)?,
            b: dd(2)?,
            a: 255,
        }),
        4 => Ok(Color {
            r: dd(0)?,
            g: dd(1)?,
            b: dd(2)?,
            a: dd(3)?,
        }),
        6 => Ok(Color {
            r: d(0, 2)?,
            g: d(2, 2)?,
            b: d(4, 2)?,
            a: 255,
        }),
        8 => Ok(Color {
            r: d(0, 2)?,
            g: d(2, 2)?,
            b: d(4, 2)?,
            a: d(6, 2)?,
        }),
        _ => Err(format!("{s} is not a color")),
    }
}

fn fill(v: &Value) -> Result<Fill, String> {
    let Value::Str(s) = v else {
        return Err(format!("expected a color or a gradient, found {}", show(v)));
    };
    let Some(inner) = s
        .strip_prefix("linear-gradient(")
        .and_then(|r| r.strip_suffix(')'))
    else {
        return color(v).map(Fill::Solid);
    };
    let wrong =
        || format!("expected \"linear-gradient(<angle>deg, <color> <stop>%, …)\", found \"{s}\"");
    let mut parts = inner.split(',').map(str::trim);
    let angle = parts
        .next()
        .and_then(|a| a.strip_suffix("deg"))
        .and_then(|a| a.trim().parse::<f32>().ok())
        .ok_or_else(wrong)?;
    let mut stops = Vec::new();
    for part in parts {
        let (c, at) = part.split_once(' ').ok_or_else(wrong)?;
        let at = at
            .trim()
            .strip_suffix('%')
            .and_then(|p| p.parse::<f32>().ok())
            .ok_or_else(wrong)?;
        stops.push((color(&Value::Str(c.to_string()))?, at / 100.0));
    }
    if stops.len() < 2 {
        return Err(wrong());
    }
    Ok(Fill::Linear { angle, stops })
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
        color: Color {
            r: 0,
            g: 0,
            b: 0,
            a: 64,
        },
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
<Frame width={390}><Button label="Pay" x={10} /></Frame>"#,
            ),
        ]);
        let scene = resolve(&fs, "a/screen.design").unwrap();
        let button = &scene.root.children[0];
        assert_eq!(button.design.as_deref(), Some("a/button.design"));
        assert_eq!(button.x, 10.0);
        assert_eq!(
            button.fill,
            Some(Fill::Solid(Color {
                r: 0x25,
                g: 0x63,
                b: 0xeb,
                a: 255
            }))
        );
        assert_eq!(button.children[0].text.as_ref().unwrap().content, "Pay");
        assert_eq!(scene.files, vec!["a/screen.design", "a/button.design"]);
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
                "screen.design:4:11: Button has no prop \"lable\"",
                "button.design:1:31: padding: expected pixels or an array of 2 or 4, found \"12px\"",
                "screen.design:5:3: row.design — no such file",
                "screen.design:6:3: unknown element Blob",
            ]
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

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

    fn problems(source: &str) -> Vec<String> {
        let fs = Mem([
            ("d/a.design".to_string(), source.as_bytes().to_vec()),
            ("d/photo.png".to_string(), PNG.to_vec()),
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

    #[test]
    fn a_valid_design_has_no_problems() {
        assert_eq!(
            problems(
                r##"import B from "./b.design"
<Frame layout="row" fill="linear-gradient(90deg, #ffffff 0%, #000000ff 100%)">
  <B label="Go" x={4} y={5} width={80} height="hug" name="Go" onClick="back" />
  <Vector d="M0 0L24 24" viewBox="0 0 24 24" stroke="#000" />
  <Image src="./photo.png" radius={[1, 2, 3, 4]} clip />
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
                ("p.png".to_string(), PNG.to_vec()),
            ]
            .into(),
        );
        let scene = resolve(&fs, "a.design").unwrap();
        let root = &scene.root;
        assert_eq!(root.padding, [1.0, 2.0, 1.0, 2.0]);
        assert_eq!(root.radius, [1.0, 2.0, 3.0, 4.0]);
        let c = |r, g, b, a| Color { r, g, b, a };
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
}
