//! Extract: one file's declarations, as its grammar's query captures them.
//! Pure and per file: what one file says, before any name is resolved
//! against another file. The query decides what is a declaration; the
//! grammar's language decides how its doc comments read.

use anyhow::{Result, anyhow};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Node, Parser, Query, QueryCursor};

/// How a declaration is visible where it is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// Visible to anyone that sees its container: `pub`.
    Public,
    /// Visible to part of the code only: `pub(crate)`, `pub(super)`.
    Restricted,
    /// Written without a visibility.
    Private,
}

/// What one file says about one thing.
#[derive(Debug, Clone)]
pub struct Declaration {
    /// The query's kind: `function`, `struct`, `field`…
    pub kind: String,
    /// Its name, as the query's `@name` captures it; for an `impl`, the type
    /// it names.
    pub name: String,
    /// How it is visible where it is written.
    pub visibility: Visibility,
    /// The declaration it is written inside, by index in the file's list.
    pub parent: Option<usize>,
    /// The declaration as written, without its body.
    pub signature: String,
    /// Its doc comment, as Markdown.
    pub doc: String,
    /// Its first line, from 1.
    pub line: usize,
    /// Whether it has a body of its own: a `mod x { … }`, not a `mod x;`.
    pub has_body: bool,
    /// Whether its attributes keep it out of the reference:
    /// `#[cfg(test)]`, `#[doc(hidden)]`.
    pub hidden: bool,
    /// For a block whose members belong to another declaration — an
    /// `impl` — what they belong to.
    pub attach: Option<Attach>,
}

/// A block whose members belong to the type it names.
#[derive(Debug, Clone)]
pub struct Attach {
    /// The type, as written: `Foo<T>`, `crate::a::Foo`.
    pub to: String,
    /// The trait it implements, as written, when it implements one.
    pub implements: Option<String>,
}

/// A statement making a name reachable where it is written: `pub use`.
#[derive(Debug, Clone)]
pub struct Reexport {
    /// How the `use` is visible where it is written.
    pub visibility: Visibility,
    /// The path as written: `a::b::C`, `crate::a::{B, C as D}`.
    pub path: String,
    /// The declaration it is written inside — an inline module — if any.
    pub parent: Option<usize>,
}

/// What one file says: its own doc comment and its declarations, in the
/// order they are written.
#[derive(Debug, Clone, Default)]
pub struct Extracted {
    /// The file's own doc comment, as Markdown.
    pub doc: String,
    /// Its declarations, in the order they are written.
    pub declarations: Vec<Declaration>,
    /// Its `pub use` statements, in the order they are written.
    pub reexports: Vec<Reexport>,
    /// The lines where the file doesn't parse; declarations elsewhere are
    /// kept.
    pub errors: Vec<usize>,
}

/// A language's parser: its grammar, the query that finds declarations, and
/// how its doc comments read.
pub struct Grammar {
    /// The tree-sitter language files are parsed with.
    language: Language,
    /// The query that captures declarations and reexports.
    query: Query,
    /// How the language's doc comments read.
    docs: Docs,
}

/// How a language writes doc comments.
#[derive(Clone, Copy)]
pub enum Docs {
    /// `///` and `/** */` before a declaration; `//!` at the top of a file.
    Rust,
}

impl Grammar {
    /// The grammar of `language` with the query `source`: an error naming the
    /// query's line when it doesn't compile against the grammar.
    pub fn new(language: Language, source: &str, docs: Docs) -> Result<Self> {
        let query = Query::new(&language, source).map_err(|error| {
            anyhow!(
                "line {}: {:?} error: {}",
                error.row + 1,
                error.kind,
                error.message
            )
        })?;
        Ok(Self {
            language,
            query,
            docs,
        })
    }

    /// The declarations of `source`.
    pub fn extract(&self, source: &str) -> Extracted {
        let mut parser = Parser::new();
        if parser.set_language(&self.language).is_err() {
            return Extracted::default();
        }
        let Some(tree) = parser.parse(source, None) else {
            return Extracted::default();
        };
        let root = tree.root_node();
        let names = self.query.capture_names();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&self.query, root, source.as_bytes());
        // Each declaration's node, to find the one it is written inside.
        let mut found: Vec<(Node, Declaration)> = Vec::new();
        let mut reexports: Vec<(Node, Reexport)> = Vec::new();
        let text = |node: Node| source[node.byte_range()].to_string();
        while let Some(found_match) = matches.next() {
            let mut item = None;
            let mut name = None;
            let mut visibility = Visibility::Private;
            let mut attach = None;
            let mut to = None;
            let mut implements = None;
            let mut reexport = None;
            let mut path = None;
            for capture in found_match.captures() {
                let capture_name = names[capture.index as usize];
                let node = capture.node;
                match capture_name {
                    "name" => name = Some(text(node)),
                    "visibility" => {
                        visibility = if text(node) == "pub" {
                            Visibility::Public
                        } else {
                            Visibility::Restricted
                        }
                    }
                    "attach" => attach = Some(node),
                    "attach.to" => to = Some(text(node)),
                    "attach.trait" => implements = Some(text(node)),
                    "reexport" => reexport = Some(node),
                    "reexport.path" => path = Some(text(node)),
                    other => {
                        if let Some(kind) = other.strip_prefix("item.") {
                            item = Some((node, kind.to_string()));
                        }
                    }
                }
            }
            if let Some((node, kind)) = item
                && let Some(name) = name
            {
                found.push((
                    node,
                    Declaration {
                        kind,
                        name,
                        visibility,
                        parent: None,
                        signature: signature(node, source),
                        doc: self.doc(node, source),
                        line: node.start_position().row + 1,
                        has_body: node.child_by_field_name("body").is_some(),
                        hidden: self.hidden(node, source),
                        attach: None,
                    },
                ));
            } else if let Some(node) = attach
                && let Some(to) = to
            {
                found.push((
                    node,
                    Declaration {
                        kind: "attach".into(),
                        name: to.clone(),
                        visibility: Visibility::Public,
                        parent: None,
                        signature: signature(node, source),
                        doc: self.doc(node, source),
                        line: node.start_position().row + 1,
                        has_body: true,
                        hidden: self.hidden(node, source),
                        attach: Some(Attach { to, implements }),
                    },
                ));
            } else if let Some(node) = reexport
                && let Some(path) = path
            {
                reexports.push((
                    node,
                    Reexport {
                        visibility,
                        path,
                        parent: None,
                    },
                ));
            }
        }
        found.sort_by_key(|(node, _)| node.start_byte());
        found.dedup_by_key(|(node, _)| node.id());
        let ids: Vec<usize> = found.iter().map(|(node, _)| node.id()).collect();
        let enclosing = |node: Node| {
            let mut at = node.parent();
            while let Some(ancestor) = at {
                if let Some(index) = ids.iter().position(|id| *id == ancestor.id()) {
                    return Some(index);
                }
                at = ancestor.parent();
            }
            None
        };
        let parents: Vec<Option<usize>> = found.iter().map(|(node, _)| enclosing(*node)).collect();
        let reexports = reexports
            .into_iter()
            .map(|(node, mut reexport)| {
                reexport.parent = enclosing(node);
                reexport
            })
            .collect();
        let declarations = found
            .into_iter()
            .zip(parents)
            .map(|((_, mut declaration), parent)| {
                declaration.parent = parent;
                declaration
            })
            .collect();
        let mut errors = Vec::new();
        errors_in(root, &mut errors);
        Extracted {
            doc: self.file_doc(root, source),
            declarations,
            reexports,
            errors,
        }
    }

    /// The doc comment written just before `node`, attributes skipped.
    fn doc(&self, node: Node, source: &str) -> String {
        match self.docs {
            Docs::Rust => {
                let mut lines = Vec::new();
                let mut at = node.prev_sibling();
                while let Some(sibling) = at {
                    match sibling.kind() {
                        "attribute_item" => {}
                        "line_comment" if sibling.child_by_field_name("outer").is_some() => {
                            let doc = sibling
                                .child_by_field_name("doc")
                                .map(|doc| &source[doc.byte_range()])
                                .unwrap_or("");
                            lines.push(comment_line(doc));
                        }
                        "block_comment" if sibling.child_by_field_name("outer").is_some() => {
                            let doc = sibling
                                .child_by_field_name("doc")
                                .map(|doc| &source[doc.byte_range()])
                                .unwrap_or("");
                            lines.push(block_text(doc));
                        }
                        _ => break,
                    }
                    at = sibling.prev_sibling();
                }
                lines.reverse();
                lines.join("\n").trim().to_string()
            }
        }
    }

    /// Whether an attribute just before `node` keeps it out of the
    /// reference: code only tests build, or code its author hid.
    fn hidden(&self, node: Node, source: &str) -> bool {
        match self.docs {
            Docs::Rust => {
                let mut at = node.prev_sibling();
                while let Some(sibling) = at {
                    match sibling.kind() {
                        "attribute_item" => {
                            let text: String = source[sibling.byte_range()]
                                .chars()
                                .filter(|c| !c.is_whitespace())
                                .collect();
                            if text == "#[cfg(test)]" || text.contains("doc(hidden)") {
                                return true;
                            }
                        }
                        "line_comment" | "block_comment" => {}
                        _ => return false,
                    }
                    at = sibling.prev_sibling();
                }
                false
            }
        }
    }

    /// The doc comment of the file itself: `//!` lines at its top.
    fn file_doc(&self, root: Node, source: &str) -> String {
        match self.docs {
            Docs::Rust => {
                let mut lines = Vec::new();
                let mut walk = root.walk();
                for child in root.children(&mut walk) {
                    match child.kind() {
                        "line_comment" if child.child_by_field_name("inner").is_some() => {
                            let doc = child
                                .child_by_field_name("doc")
                                .map(|doc| &source[doc.byte_range()])
                                .unwrap_or("");
                            lines.push(comment_line(doc));
                        }
                        "line_comment" | "block_comment" | "inner_attribute_item" => {}
                        _ => break,
                    }
                }
                lines.join("\n").trim().to_string()
            }
        }
    }
}

/// A declaration as written without its body: everything before the node's
/// `body` field, or the whole node when it has none.
fn signature(node: Node, source: &str) -> String {
    let end = node
        .child_by_field_name("body")
        .map_or(node.end_byte(), |body| body.start_byte());
    let text = source[node.start_byte()..end].trim_end();
    let text = text.strip_suffix(';').unwrap_or(text).trim_end();
    dedent(text)
}

/// `text` with the indentation its later lines share taken off, so a
/// signature split over lines reads as written at the top level.
fn dedent(text: &str) -> String {
    let mut lines = text.lines();
    let first = lines.next().unwrap_or("").to_string();
    let rest: Vec<&str> = lines.collect();
    let indent = rest
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    // Later lines keep the indent they have beyond the first line's.
    let keep = indent.saturating_sub(4);
    std::iter::once(first)
        .chain(rest.iter().map(|line| {
            line.get(indent - keep..)
                .unwrap_or(line.trim_start())
                .to_string()
        }))
        .collect::<Vec<_>>()
        .join("\n")
}

/// One `///` or `//!` line's text: the space after the marker taken off.
fn comment_line(doc: &str) -> String {
    let doc = doc.strip_suffix('\n').unwrap_or(doc);
    let doc = doc.strip_suffix('\r').unwrap_or(doc);
    doc.strip_prefix(' ').unwrap_or(doc).to_string()
}

/// A `/** */` comment's text: each line's leading `*` taken off.
fn block_text(doc: &str) -> String {
    doc.lines()
        .map(|line| {
            let line = line.trim_start();
            let line = line.strip_prefix('*').unwrap_or(line);
            line.strip_prefix(' ').unwrap_or(line)
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// The lines of every part of the tree that doesn't parse.
fn errors_in(node: Node, errors: &mut Vec<usize>) {
    if node.is_error() || node.is_missing() {
        errors.push(node.start_position().row + 1);
        return;
    }
    if !node.has_error() {
        return;
    }
    let mut walk = node.walk();
    for child in node.children(&mut walk) {
        errors_in(child, errors);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rust() -> Grammar {
        Grammar::new(
            tree_sitter_rust::LANGUAGE.into(),
            include_str!("rust.scm"),
            Docs::Rust,
        )
        .unwrap()
    }

    #[test]
    fn a_rust_file_says_what_it_declares_inside_what_with_its_docs() {
        let extracted = rust().extract(
            "//! The module.\n\n/// A thing.\n/// Two lines.\n#[derive(Debug)]\npub struct Foo<T> {\n    /// The a.\n    pub a: u8,\n    b: T,\n}\n\nimpl<T> Foo<T> {\n    /// Reads.\n    pub fn read(&self, id: u32) -> u8 {\n        fn local() {}\n        1\n    }\n}\n\nimpl Display for Foo<u8> {}\n\npub(crate) mod inner;\npub use inner::Thing;\n",
        );
        assert_eq!(extracted.doc, "The module.");
        let shown: Vec<_> = extracted
            .declarations
            .iter()
            .map(|d| (d.kind.as_str(), d.name.as_str(), d.parent, d.visibility))
            .collect();
        assert_eq!(
            shown,
            [
                ("struct", "Foo", None, Visibility::Public),
                ("field", "a", Some(0), Visibility::Public),
                ("field", "b", Some(0), Visibility::Private),
                ("attach", "Foo<T>", None, Visibility::Public),
                ("function", "read", Some(3), Visibility::Public),
                ("function", "local", Some(4), Visibility::Private),
                ("attach", "Foo<u8>", None, Visibility::Public),
                ("module", "inner", None, Visibility::Restricted),
            ]
        );
        let foo = &extracted.declarations[0];
        assert_eq!(foo.doc, "A thing.\nTwo lines.");
        assert_eq!(foo.signature, "pub struct Foo<T>");
        assert_eq!(foo.line, 6);
        assert_eq!(extracted.declarations[1].doc, "The a.");
        assert_eq!(
            extracted.declarations[4].signature,
            "pub fn read(&self, id: u32) -> u8"
        );
        assert_eq!(
            extracted.declarations[6]
                .attach
                .as_ref()
                .unwrap()
                .implements
                .as_deref(),
            Some("Display")
        );
        assert!(!extracted.declarations[7].has_body);
        assert_eq!(extracted.reexports.len(), 1);
        assert_eq!(extracted.reexports[0].path, "inner::Thing");
        assert!(extracted.errors.is_empty());
    }

    #[test]
    fn a_query_that_does_not_compile_names_its_line() {
        let error = Grammar::new(
            tree_sitter_rust::LANGUAGE.into(),
            "(function_item) @item.function\n(no_such_node) @item.x\n",
            Docs::Rust,
        )
        .err()
        .unwrap();
        assert!(error.to_string().starts_with("line 2:"), "{error}");
    }

    #[test]
    fn a_file_that_does_not_parse_keeps_what_does() {
        let extracted = rust().extract("pub fn ok() {}\npub fn broken( {\n");
        assert_eq!(extracted.declarations[0].name, "ok");
        assert!(!extracted.errors.is_empty());
    }
}
