//! Text → syntax tree: the JSX subset a `.design` file is written in.

use crate::Problem;

/// A line and column, both from 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct File {
    pub imports: Vec<Import>,
    pub root: Element,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    pub name: String,
    pub path: String,
    pub pos: Pos,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<Attr>,
    pub children: Vec<Child>,
    pub pos: Pos,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Child {
    Element(Element),
    Text(Vec<TextPart>, Pos),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TextPart {
    Literal(String),
    Prop(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attr {
    pub name: String,
    pub value: Value,
    pub pos: Pos,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Num(f64),
    Bool(bool),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
    Prop(String),
}

/// Parses a whole file, or gives the one problem where it first went wrong.
pub fn parse(source: &str) -> Result<File, Problem> {
    let mut p = Parser {
        chars: source.chars().collect(),
        at: 0,
        line: 1,
        col: 1,
        depth: 0,
    };
    let mut imports = Vec::new();
    p.skip_space();
    while p.peek_word("import") {
        let pos = p.pos();
        p.expect_word("import")?;
        p.skip_space();
        let name = p.ident()?;
        p.skip_space();
        p.expect_word("from")?;
        p.skip_space();
        let path = p.string()?;
        imports.push(Import { name, path, pos });
        p.skip_space();
    }
    if p.peek() != Some('<') {
        return Err(p.problem("expected an import or the root element"));
    }
    let root = p.element()?;
    p.skip_space();
    if p.peek().is_some() {
        return Err(p.problem("expected nothing after the root element"));
    }
    Ok(File { imports, root })
}

struct Parser {
    chars: Vec<char>,
    at: usize,
    line: u32,
    col: u32,
    /// How many elements, arrays and objects enclose the current place.
    depth: usize,
}

/// How deep elements, arrays and objects may nest in one file: far beyond any
/// real design, and well inside the stack the parser recurses on.
pub const MAX_DEPTH: usize = 256;

/// The largest number a design may write, in either direction: far beyond any
/// real design, and small enough that layout and painting stay finite.
pub const MAX_NUMBER: f64 = 1_000_000.0;

impl Parser {
    fn pos(&self) -> Pos {
        Pos {
            line: self.line,
            col: self.col,
        }
    }

    fn problem(&self, message: &str) -> Problem {
        Problem {
            file: String::new(),
            pos: self.pos(),
            message: message.to_string(),
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.chars.get(self.at + n).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.at += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    /// Skips whitespace and `//` comments.
    fn skip_space(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => {
                    self.bump();
                }
                Some('/') if self.peek_at(1) == Some('/') => {
                    while !matches!(self.peek(), None | Some('\n')) {
                        self.bump();
                    }
                }
                _ => return,
            }
        }
    }

    fn peek_word(&self, word: &str) -> bool {
        word.chars()
            .enumerate()
            .all(|(i, c)| self.peek_at(i) == Some(c))
            && !self
                .peek_at(word.len())
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
    }

    fn expect_word(&mut self, word: &str) -> Result<(), Problem> {
        if !self.peek_word(word) {
            return Err(self.problem(&format!("expected `{word}`")));
        }
        for _ in 0..word.len() {
            self.bump();
        }
        Ok(())
    }

    fn expect(&mut self, c: char) -> Result<(), Problem> {
        if self.peek() != Some(c) {
            return Err(self.problem(&format!("expected `{c}`")));
        }
        self.bump();
        Ok(())
    }

    fn ident(&mut self) -> Result<String, Problem> {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' || (c == '-' && !s.is_empty()) {
                s.push(c);
                self.bump();
            } else {
                break;
            }
        }
        if s.is_empty() || s.starts_with(|c: char| c.is_ascii_digit()) {
            return Err(self.problem("expected a name"));
        }
        Ok(s)
    }

    /// Enters one more level of nesting, or says the file nests too deep.
    fn deeper(&mut self) -> Result<(), Problem> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.problem(&format!("nested more than {MAX_DEPTH} deep")));
        }
        Ok(())
    }

    /// A quoted string, JSX's way: double or single quotes, and no escapes —
    /// a backslash is itself.
    fn string(&mut self) -> Result<String, Problem> {
        let quote = match self.peek() {
            Some(q @ ('"' | '\'')) => q,
            _ => return Err(self.problem("expected a string")),
        };
        // An unterminated string is reported where it opened.
        let open = self.problem("unterminated string");
        self.bump();
        let mut s = String::new();
        loop {
            match self.bump() {
                None | Some('\n') => return Err(open),
                Some(c) if c == quote => return Ok(s),
                Some(c) => s.push(c),
            }
        }
    }

    fn element(&mut self) -> Result<Element, Problem> {
        let pos = self.pos();
        self.deeper()?;
        let element = self.element_inner(pos);
        self.depth -= 1;
        element
    }

    fn element_inner(&mut self, pos: Pos) -> Result<Element, Problem> {
        self.expect('<')?;
        let name = self.ident()?;
        let mut attrs = Vec::new();
        loop {
            self.skip_space();
            match self.peek() {
                Some('/') => {
                    self.bump();
                    self.expect('>')?;
                    return Ok(Element {
                        name,
                        attrs,
                        children: Vec::new(),
                        pos,
                    });
                }
                Some('>') => {
                    self.bump();
                    break;
                }
                _ => {
                    let pos = self.pos();
                    let attr = self.ident()?;
                    self.skip_space();
                    let value = if self.peek() == Some('=') {
                        self.bump();
                        self.skip_space();
                        self.attr_value()?
                    } else {
                        Value::Bool(true)
                    };
                    attrs.push(Attr {
                        name: attr,
                        value,
                        pos,
                    });
                }
            }
        }
        let children = self.children(&name)?;
        Ok(Element {
            name,
            attrs,
            children,
            pos,
        })
    }

    fn children(&mut self, parent: &str) -> Result<Vec<Child>, Problem> {
        let mut children = Vec::new();
        let mut text: Vec<TextPart> = Vec::new();
        let mut text_pos = self.pos();
        let mut literal = String::new();
        loop {
            match self.peek() {
                None => return Err(self.problem(&format!("`<{parent}>` is never closed"))),
                Some('<') => {
                    flush_text(&mut literal, &mut text, &mut children, text_pos);
                    if self.peek_at(1) == Some('/') {
                        self.bump();
                        self.bump();
                        let name = self.ident()?;
                        if name != parent {
                            return Err(self.problem(&format!("`</{name}>` closes `<{parent}>`")));
                        }
                        self.skip_space();
                        self.expect('>')?;
                        return Ok(children);
                    }
                    children.push(Child::Element(self.element()?));
                    text_pos = self.pos();
                }
                Some('{') => {
                    if text.is_empty() && literal.trim().is_empty() {
                        text_pos = self.pos();
                    }
                    if !literal.is_empty() {
                        text.push(TextPart::Literal(std::mem::take(&mut literal)));
                    }
                    self.bump();
                    self.skip_space();
                    let name = self.ident()?;
                    self.skip_space();
                    self.expect('}')?;
                    text.push(TextPart::Prop(name));
                }
                Some(c) => {
                    if text.is_empty() && literal.trim().is_empty() && !c.is_whitespace() {
                        text_pos = self.pos();
                    }
                    literal.push(c);
                    self.bump();
                }
            }
        }
    }

    fn attr_value(&mut self) -> Result<Value, Problem> {
        match self.peek() {
            Some('"' | '\'') => Ok(Value::Str(self.string()?)),
            Some('{') => {
                self.bump();
                self.skip_space();
                let value = self.value()?;
                self.skip_space();
                self.expect('}')?;
                Ok(value)
            }
            _ => Err(self.problem("expected a string or a value in braces")),
        }
    }

    fn value(&mut self) -> Result<Value, Problem> {
        self.deeper()?;
        let value = self.value_inner();
        self.depth -= 1;
        value
    }

    fn value_inner(&mut self) -> Result<Value, Problem> {
        match self.peek() {
            Some('"' | '\'') => Ok(Value::Str(self.string()?)),
            Some('[') => {
                self.bump();
                let mut items = Vec::new();
                loop {
                    self.skip_space();
                    if self.peek() == Some(']') {
                        self.bump();
                        return Ok(Value::Array(items));
                    }
                    items.push(self.value()?);
                    self.skip_space();
                    match self.peek() {
                        Some(',') => {
                            self.bump();
                        }
                        Some(']') => {}
                        _ => return Err(self.problem("expected `,` or `]`")),
                    }
                }
            }
            Some('{') => {
                self.bump();
                let mut fields = Vec::new();
                loop {
                    self.skip_space();
                    if self.peek() == Some('}') {
                        self.bump();
                        return Ok(Value::Object(fields));
                    }
                    let key = self.ident()?;
                    self.skip_space();
                    self.expect(':')?;
                    self.skip_space();
                    fields.push((key, self.value()?));
                    self.skip_space();
                    match self.peek() {
                        Some(',') => {
                            self.bump();
                        }
                        Some('}') => {}
                        _ => return Err(self.problem("expected `,` or `}`")),
                    }
                }
            }
            Some(c) if c.is_ascii_digit() || c == '-' || c == '.' => {
                let mut s = String::new();
                while let Some(c) = self.peek() {
                    if c.is_ascii_digit() || c == '-' || c == '.' {
                        s.push(c);
                        self.bump();
                    } else {
                        break;
                    }
                }
                match s.parse::<f64>() {
                    Ok(n) if n.abs() <= MAX_NUMBER => Ok(Value::Num(n)),
                    Ok(_) => Err(self.problem(&format!(
                        "`{s}` is too large: numbers go up to {MAX_NUMBER}"
                    ))),
                    Err(_) => Err(self.problem(&format!("`{s}` is not a number"))),
                }
            }
            _ => {
                let name = self.ident()?;
                Ok(match name.as_str() {
                    "true" => Value::Bool(true),
                    "false" => Value::Bool(false),
                    _ => Value::Prop(name),
                })
            }
        }
    }
}

/// Ends a run of text: JSX rules, so whitespace with a newline in it vanishes
/// and other runs of whitespace become one space.
fn flush_text(literal: &mut String, text: &mut Vec<TextPart>, children: &mut Vec<Child>, pos: Pos) {
    if !literal.is_empty() {
        text.push(TextPart::Literal(std::mem::take(literal)));
    }
    let mut parts = Vec::new();
    for part in text.drain(..) {
        match part {
            TextPart::Literal(s) => {
                let s = collapse(&s);
                if !s.is_empty() {
                    parts.push(TextPart::Literal(s));
                }
            }
            prop => parts.push(prop),
        }
    }
    if !parts.is_empty() {
        children.push(Child::Text(parts, pos));
    }
}

fn collapse(s: &str) -> String {
    let lines: Vec<&str> = s.split('\n').collect();
    if lines.len() == 1 {
        return s.to_string();
    }
    lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_attributes_and_text_with_props() {
        let file = parse(
            r##"// a comment
import Button from "./button.design"

<Frame width={390} padding={[12, 20]} shadow={{ y: 4, color: "#0002" }} clip>
  <Text size={16}>Hi, {name}</Text>
  <Button label="Pay" />
</Frame>"##,
        )
        .unwrap();
        assert_eq!(file.imports[0].name, "Button");
        assert_eq!(file.imports[0].path, "./button.design");
        let root = &file.root;
        assert_eq!(root.attrs[0].value, Value::Num(390.0));
        assert_eq!(
            root.attrs[1].value,
            Value::Array(vec![Value::Num(12.0), Value::Num(20.0)])
        );
        assert!(matches!(root.attrs[2].value, Value::Object(ref f) if f.len() == 2));
        assert_eq!(root.attrs[3].value, Value::Bool(true));
        let Child::Element(text) = &root.children[0] else {
            panic!()
        };
        assert_eq!(
            text.children,
            vec![Child::Text(
                vec![
                    TextPart::Literal("Hi, ".into()),
                    TextPart::Prop("name".into())
                ],
                Pos { line: 5, col: 19 }
            )]
        );
        assert_eq!(root.children.len(), 2);
    }

    #[test]
    fn a_problem_has_its_place() {
        let problem = parse("<Frame>\n  <Text>hi</Frame>").unwrap_err();
        assert_eq!(problem.pos, Pos { line: 2, col: 18 });
        assert_eq!(problem.message, "`</Frame>` closes `<Text>`");
    }

    #[test]
    fn every_syntax_error_has_its_place() {
        let cases = [
            ("", "1:1: expected an import or the root element"),
            (
                "<Frame />\n<Frame />",
                "2:1: expected nothing after the root element",
            ),
            ("import \"./a.design\"", "1:8: expected a name"),
            ("import A \"./a.design\"", "1:10: expected `from`"),
            ("import A from ./a.design", "1:15: expected a string"),
            ("<Frame name=\"x\n/>", "1:13: unterminated string"),
            (
                "<Frame>\n  <Text>hi</Text>",
                "2:18: `<Frame>` is never closed",
            ),
            (
                "<Frame gap=4 />",
                "1:12: expected a string or a value in braces",
            ),
            ("<Frame padding={[1 2]} />", "1:20: expected `,` or `]`"),
            ("<Frame shadow={{ y 4 }} />", "1:20: expected `:`"),
            (
                "<Frame shadow={{ y: 4 x: 1 }} />",
                "1:23: expected `,` or `}`",
            ),
            ("<Frame gap={1.2.3} />", "1:18: `1.2.3` is not a number"),
            ("<Frame gap={4 />", "1:15: expected `}`"),
            ("<Frame><Text>{ }</Text></Frame>", "1:16: expected a name"),
            (
                "<Frame><Text>hi</Frame>",
                "1:23: `</Frame>` closes `<Text>`",
            ),
            ("<Frame / >", "1:9: expected `>`"),
        ];
        for (source, expected) in cases {
            let problem = parse(source).unwrap_err();
            assert_eq!(
                format!(
                    "{}:{}: {}",
                    problem.pos.line, problem.pos.col, problem.message
                ),
                expected,
                "{source:?}"
            );
        }
    }

    #[test]
    fn strings_bare_attributes_and_names_follow_jsx() {
        let file = parse(r#"<Frame name='it\s "quoted"' aria-label="a\nb" clip data-x={false} />"#)
            .unwrap();
        let attrs = &file.root.attrs;
        // No escapes: a backslash is itself, in either quote.
        assert_eq!(attrs[0].value, Value::Str(r#"it\s "quoted""#.into()));
        assert_eq!(attrs[1].name, "aria-label");
        assert_eq!(attrs[1].value, Value::Str(r"a\nb".into()));
        // A bare attribute is true.
        assert_eq!(attrs[2].value, Value::Bool(true));
        assert_eq!(attrs[3].name, "data-x");
        assert_eq!(attrs[3].value, Value::Bool(false));
    }

    #[test]
    fn nesting_and_numbers_have_limits() {
        let deep = "<Frame>".repeat(MAX_DEPTH + 1) + &"</Frame>".repeat(MAX_DEPTH + 1);
        let problem = parse(&deep).unwrap_err();
        assert_eq!(
            problem.message,
            format!("nested more than {MAX_DEPTH} deep")
        );
        let deep = format!("<Frame gap={{{}}} />", "[".repeat(100_000));
        let problem = parse(&deep).unwrap_err();
        assert_eq!(
            problem.message,
            format!("nested more than {MAX_DEPTH} deep")
        );
        let problem = parse(&format!("<Frame width={{{}}} />", "9".repeat(400))).unwrap_err();
        assert!(
            problem
                .message
                .ends_with("is too large: numbers go up to 1000000"),
            "{}",
            problem.message
        );
        assert!(parse("<Frame width={1000000} />").is_ok());
        let fine = "<Frame>".repeat(MAX_DEPTH) + &"</Frame>".repeat(MAX_DEPTH);
        assert!(parse(&fine).is_ok());
    }

    #[test]
    fn text_follows_jsx_whitespace() {
        let file = parse("<Text>\n    Free shipping\n    on orders\n  </Text>").unwrap();
        assert_eq!(
            file.root.children,
            vec![Child::Text(
                vec![TextPart::Literal("Free shipping on orders".into())],
                Pos { line: 2, col: 5 }
            )]
        );
    }
}
