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
    pub fill: Option<Color>,
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
            design: None,
            children: Vec::new(),
        }
    }
}

/// Reads a design's file by its path from the top of the served folder.
pub trait Files {
    fn read(&self, path: &str) -> Option<String>;
}

impl Files for HashMap<String, String> {
    fn read(&self, path: &str) -> Option<String> {
        self.get(path).cloned()
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
            imports.insert(import.name.clone(), join(path, &import.path));
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
                        if matches!(kind, Kind::Rect | Kind::Ellipse) {
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

    fn attribute(&mut self, path: &str, node: &mut Node, attr: &parse::Attr, value: Value) {
        let kind = node.kind;
        let ok = match (attr.name.as_str(), kind) {
            ("x" | "y" | "width" | "height" | "name" | "opacity" | "onClick", _) => {
                self.common(path, node, attr, value);
                return;
            }
            ("fill", Kind::Frame | Kind::Rect | Kind::Ellipse) => {
                color(&value).map(|c| node.fill = Some(c))
            }
            ("stroke", Kind::Frame | Kind::Rect | Kind::Ellipse) => {
                color(&value).map(|c| node.stroke = Some(c))
            }
            ("strokeWidth", Kind::Frame | Kind::Rect | Kind::Ellipse) => {
                num(&value).map(|n| node.stroke_width = n)
            }
            ("radius", Kind::Frame | Kind::Rect) => match &value {
                Value::Num(n) => Ok(node.radius = [*n as f32; 4]),
                Value::Array(a) if a.len() == 4 => {
                    nums(a).map(|v| node.radius = [v[0], v[1], v[2], v[3]])
                }
                _ => Err("expected pixels or an array of 4".into()),
            },
            ("shadow", Kind::Frame | Kind::Rect | Kind::Ellipse) => {
                shadow(&value).map(|s| node.shadow = Some(s))
            }
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
    }
}

fn interaction_ok(s: &str) -> bool {
    s == "back"
        || s == "close"
        || s.strip_prefix("navigate ")
            .or(s.strip_prefix("overlay "))
            .is_some_and(|p| !p.trim().is_empty())
}

/// A path relative to a design's file, from the top of the served folder.
pub fn join(from: &str, relative: &str) -> String {
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop();
    for part in relative.split('/') {
        match part {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
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
            Some(Color {
                r: 0x25,
                g: 0x63,
                b: 0xeb,
                a: 255
            })
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
}
