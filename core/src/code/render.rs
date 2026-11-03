//! Render: a reference page as Markdown, laid out as DocC lays out a symbol
//! page, which the page pipeline then shows in its site's frame like any page
//! of the site. Every page opens with where it is, its kind, its name and its
//! abstract; then its declaration, the rest of its doc as an overview, the
//! prose that adds to it, its Topics — what it holds, grouped by kind, each a
//! link with its abstract — and its relationships.

use std::path::Path;

use super::{Index, Member, Prose, Target, Targets};
use crate::html::escape;

/// The Topics groups of a module's page, in order: a kind and its heading.
const ITEMS: [(&str, &str); 10] = [
    ("module", "Modules"),
    ("struct", "Structures"),
    ("enum", "Enumerations"),
    ("union", "Unions"),
    ("trait", "Traits"),
    ("type", "Type Aliases"),
    ("function", "Functions"),
    ("constant", "Constants"),
    ("static", "Statics"),
    ("macro", "Macros"),
];

/// The page of a namespace: every crate it holds.
pub fn namespace(index: &Index, name: &str, targets: &Targets) -> String {
    let mut out = format!(
        "{}\n\n# {} reference\n\n",
        eyebrow("Reference"),
        title_case(name)
    );
    let crates: Vec<&Target> = targets
        .targets
        .values()
        .filter(|target| target.path.len() == 1)
        .collect();
    if crates.is_empty() {
        out.push_str("No code matches this namespace's `files`.\n");
        return out;
    }
    out.push_str("## Topics\n\n### Crates\n\n");
    for krate in crates {
        out.push_str(&topic(
            &format!("`{}`", krate.path[0]),
            &index.address(name, &krate.path),
            &krate.doc,
        ));
    }
    out
}

/// The page of a module, a type or a function.
pub fn target(
    index: &Index,
    namespace: &str,
    targets: &Targets,
    target: &Target,
    prose: Option<&Prose>,
) -> String {
    let name = target.path.last().cloned().unwrap_or_default();
    let kind = if target.path.len() == 1 {
        "Crate"
    } else {
        kind_label(&target.kind)
    };
    let mut out = breadcrumbs(index, namespace, &target.path[..target.path.len() - 1]);
    out.push_str(&format!("{}\n\n# {name}\n\n", eyebrow(kind)));
    let (summary, overview) = split_doc(&target.doc);
    out.push_str(&abstract_of(&summary));
    if target.kind != "module" && !target.signature.is_empty() {
        out.push_str("## Declaration\n\n");
        out.push_str(&fence(&target.signature));
    }
    out.push_str(&declared(index, &target.file, target.line));
    out.push_str(&overview_of(&overview, prose));

    let mut topics = String::new();
    if target.kind == "module" {
        let children: Vec<&Target> = targets
            .targets
            .values()
            .filter(|child| {
                child.path.len() == target.path.len() + 1 && child.path.starts_with(&target.path)
            })
            .collect();
        for (kind, heading) in ITEMS {
            let listed: Vec<&&Target> = children.iter().filter(|c| c.kind == kind).collect();
            if listed.is_empty() {
                continue;
            }
            topics.push_str(&format!("### {heading}\n\n"));
            for child in listed {
                let label = if child.kind == "module" {
                    format!("`{}`", child.path.last().unwrap())
                } else {
                    format!("`{}`", short(&child.signature))
                };
                topics.push_str(&topic(
                    &label,
                    &index.address(namespace, &child.path),
                    &child.doc,
                ));
            }
        }
    }
    for (group, heading) in [
        (Group::Field, "Fields"),
        (Group::Variant, "Variants"),
        (Group::Function, "Associated Functions"),
        (Group::Method, "Methods"),
    ] {
        let members: Vec<&Member> = target
            .members
            .iter()
            .filter(|member| group_of(member) == group)
            .collect();
        if members.is_empty() {
            continue;
        }
        topics.push_str(&format!("### {heading}\n\n"));
        for member in members {
            let mut path = target.path.clone();
            path.push(member.name.clone());
            topics.push_str(&topic(
                &format!("`{}`", short(&member.signature)),
                &index.address(namespace, &path),
                &member.doc,
            ));
        }
    }
    if !topics.is_empty() {
        out.push_str("## Topics\n\n");
        out.push_str(&topics);
    }
    if !target.implements.is_empty() {
        out.push_str("## Relationships\n\n### Implements\n\n");
        for name in &target.implements {
            out.push_str(&format!("- `{name}`\n"));
        }
        out.push('\n');
    }
    out
}

/// The page of a field, a variant or a method of `owner`.
pub fn member(
    index: &Index,
    namespace: &str,
    owner: &Target,
    member: &Member,
    prose: Option<&Prose>,
) -> String {
    let mut out = breadcrumbs(index, namespace, &owner.path);
    out.push_str(&format!(
        "{}\n\n# {}\n\n",
        eyebrow(group_of(member).label()),
        member.name
    ));
    let (summary, overview) = split_doc(&member.doc);
    out.push_str(&abstract_of(&summary));
    out.push_str("## Declaration\n\n");
    out.push_str(&fence(&member.signature));
    out.push_str(&declared(index, &member.file, member.line));
    out.push_str(&overview_of(&overview, prose));
    out
}

/// What a member is, as a Topics group of its type's page.
#[derive(Clone, Copy, PartialEq)]
enum Group {
    Field,
    Variant,
    /// A function of the type that takes no `self`.
    Function,
    Method,
}

impl Group {
    fn label(self) -> &'static str {
        match self {
            Group::Field => "Field",
            Group::Variant => "Variant",
            Group::Function => "Associated Function",
            Group::Method => "Method",
        }
    }
}

fn group_of(member: &Member) -> Group {
    match member.kind.as_str() {
        "field" => Group::Field,
        "variant" => Group::Variant,
        _ if takes_self(&member.signature) => Group::Method,
        _ => Group::Function,
    }
}

/// Whether a function's first parameter is `self`: `self`, `&self`,
/// `&'a mut self`, `mut self: Box<Self>`.
fn takes_self(signature: &str) -> bool {
    let Some(open) = signature.find('(') else {
        return false;
    };
    let mut first = signature[open + 1..].trim_start();
    first = first.strip_prefix('&').unwrap_or(first).trim_start();
    if first.starts_with('\'') {
        first = first
            .trim_start_matches(|c: char| c == '\'' || c.is_alphanumeric() || c == '_')
            .trim_start();
    }
    first = first.strip_prefix("mut ").unwrap_or(first).trim_start();
    first
        .strip_prefix("self")
        .is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
}

/// A link back up the path: the namespace, then each module.
fn breadcrumbs(index: &Index, namespace: &str, path: &[String]) -> String {
    let mut links = vec![format!(
        r#"<a href="{}">{}</a>"#,
        escape(&index.address(namespace, &[])),
        escape(&title_case(namespace))
    )];
    for at in 1..=path.len() {
        links.push(format!(
            r#"<a href="{}">{}</a>"#,
            escape(&index.address(namespace, &path[..at])),
            escape(&path[at - 1])
        ));
    }
    format!(
        "<nav class=\"code-breadcrumbs\">{}</nav>\n\n",
        links.join(" › ")
    )
}

fn eyebrow(kind: &str) -> String {
    format!("<p class=\"code-eyebrow\">{}</p>", escape(kind))
}

/// The abstract under the title, larger than the text.
fn abstract_of(summary: &str) -> String {
    if summary.is_empty() {
        return "<div class=\"code-abstract code-undocumented\">\n\nNo overview available.\n\n</div>\n\n"
            .to_string();
    }
    format!("<div class=\"code-abstract\">\n\n{summary}\n\n</div>\n\n")
}

/// The rest of the doc, and the prose that adds to it, under Overview.
fn overview_of(overview: &str, prose: Option<&Prose>) -> String {
    let prose = prose.map(|prose| prose.body.trim()).unwrap_or("");
    if overview.is_empty() && prose.is_empty() {
        return String::new();
    }
    let mut out = String::from("## Overview\n\n");
    for part in [overview, prose] {
        if !part.is_empty() {
            out.push_str(part);
            out.push_str("\n\n");
        }
    }
    out
}

/// "Declared in `file`, line n", linking the file as the served folder
/// serves it.
fn declared(index: &Index, file: &Path, line: usize) -> String {
    match index.source(file) {
        Some((address, name)) => format!(
            "<div class=\"code-declared\">\n\nDeclared in [{name}]({address}), line {line}\n\n</div>\n\n"
        ),
        None => String::new(),
    }
}

/// One entry of a Topics group: its label linked, its abstract under it.
fn topic(label: &str, address: &str, doc: &str) -> String {
    let (summary, _) = split_doc(doc);
    let summary = summary.split("\n").collect::<Vec<_>>().join(" ");
    if summary.is_empty() {
        format!("<div class=\"code-topic\">\n\n[{label}]({address})\n\n</div>\n\n")
    } else {
        format!("<div class=\"code-topic\">\n\n[{label}]({address})\n\n{summary}\n\n</div>\n\n")
    }
}

/// A declaration on one line, without its visibility: what a Topics entry
/// shows.
fn short(signature: &str) -> String {
    let line = signature.split_whitespace().collect::<Vec<_>>().join(" ");
    let line = line
        .replace("( ", "(")
        .replace(" )", ")")
        .replace(",)", ")");
    match line.strip_prefix("pub") {
        Some(rest) if rest.starts_with(' ') => rest.trim_start().to_string(),
        Some(rest) if rest.starts_with('(') => match rest.find(')') {
            Some(close) => rest[close + 1..].trim_start().to_string(),
            None => line.clone(),
        },
        _ => line.clone(),
    }
}

/// A doc comment as its abstract — its first paragraph — and the rest, both
/// as page Markdown. A doc opening with a heading or code has no abstract.
fn split_doc(doc: &str) -> (String, String) {
    let markdown = doc_markdown(doc);
    let mut lines = markdown.lines();
    let mut summary = Vec::new();
    for line in lines.by_ref() {
        if line.trim().is_empty() {
            if summary.is_empty() {
                continue;
            }
            break;
        }
        if summary.is_empty() && (line.starts_with('#') || line.trim_start().starts_with("```")) {
            return (String::new(), markdown.trim().to_string());
        }
        summary.push(line);
    }
    let rest: Vec<&str> = lines.collect();
    (summary.join("\n"), rest.join("\n").trim().to_string())
}

fn kind_label(kind: &str) -> &str {
    match kind {
        "module" => "Module",
        "struct" => "Structure",
        "enum" => "Enumeration",
        "union" => "Union",
        "trait" => "Trait",
        "type" => "Type Alias",
        "function" => "Function",
        "constant" => "Constant",
        "static" => "Static",
        "macro" => "Macro",
        other => other,
    }
}

fn title_case(name: &str) -> String {
    let mut chars = name.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

/// A Rust code block of `code`, long enough to hold any backticks in it.
fn fence(code: &str) -> String {
    let ticks = "`".repeat(longest_run(code, '`').max(2) + 1);
    format!("{ticks}rust\n{code}\n{ticks}\n\n")
}

fn longest_run(text: &str, c: char) -> usize {
    let mut longest = 0;
    let mut run = 0;
    for ch in text.chars() {
        run = if ch == c { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    longest
}

/// A doc comment as page Markdown: its headings below the page's own, an
/// unlabelled code block as Rust with rustdoc's hidden `# ` lines left out,
/// and rustdoc's links to names shown as the name.
fn doc_markdown(doc: &str) -> String {
    let mut out = Vec::new();
    let mut fence: Option<(String, bool)> = None;
    for line in doc.lines() {
        let trimmed = line.trim_start();
        if let Some((marker, rust)) = &fence {
            if trimmed.starts_with(marker.as_str())
                && trimmed.trim_start_matches('`').trim().is_empty()
            {
                out.push(line.to_string());
                fence = None;
                continue;
            }
            if *rust && (trimmed == "#" || trimmed.starts_with("# ")) {
                continue;
            }
            out.push(line.to_string());
            continue;
        }
        if trimmed.starts_with("```") {
            let ticks = trimmed.len() - trimmed.trim_start_matches('`').len();
            let label = trimmed.trim_start_matches('`').trim();
            let rust = label.is_empty()
                || label.split(',').any(|part| {
                    matches!(
                        part.trim(),
                        "rust" | "ignore" | "no_run" | "should_panic" | "compile_fail"
                    )
                });
            fence = Some(("`".repeat(ticks), rust));
            out.push(if label.is_empty() {
                format!("{line}rust")
            } else if rust {
                format!("{}rust", &line[..line.len() - label.len()])
            } else {
                line.to_string()
            });
            continue;
        }
        if trimmed.starts_with('#') {
            out.push(format!("##{trimmed}"));
            continue;
        }
        out.push(intra_doc_links(line));
    }
    out.join("\n")
}

/// rustdoc's links to names — ``[`Foo`]``, ``[`Foo`](crate::a::Foo)`` — as
/// the name in code, since a Rust path is no address.
fn intra_doc_links(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(open) = rest.find("[`") {
        let Some(close) = rest[open..].find("`]") else {
            break;
        };
        let close = open + close;
        let name = &rest[open + 2..close];
        let after = &rest[close + 2..];
        out.push_str(&rest[..open]);
        if let Some(target) = after.strip_prefix('(')
            && let Some(end) = target.find(')')
        {
            let href = &target[..end];
            if href.contains("::") || !href.contains(['/', '.', '#']) {
                out.push_str(&format!("`{name}`"));
                rest = &target[end + 1..];
                continue;
            }
            out.push_str(&rest[open..close + 2]);
            rest = after;
            continue;
        }
        if after.starts_with('[') {
            out.push_str(&rest[open..close + 2]);
            rest = after;
            continue;
        }
        out.push_str(&format!("`{name}`"));
        rest = after;
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_doc_splits_into_its_abstract_and_the_rest() {
        assert_eq!(
            split_doc("Holds entries.\nTwo lines.\n\n# Use\n\nMore."),
            (
                "Holds entries.\nTwo lines.".to_string(),
                "### Use\n\nMore.".to_string()
            )
        );
        assert_eq!(split_doc("```\nx\n```").0, "");
    }

    #[test]
    fn a_topic_shows_a_declaration_on_one_line_without_its_visibility() {
        assert_eq!(
            short("pub fn new(\n    folder: &'a Folder,\n    comments: bool,\n) -> Self"),
            "fn new(folder: &'a Folder, comments: bool) -> Self"
        );
        assert_eq!(short("pub(crate) struct Store<T>"), "struct Store<T>");
    }

    #[test]
    fn a_function_taking_self_is_a_method() {
        assert!(takes_self("pub fn read(&self) -> u8"));
        assert!(takes_self("fn take(mut self)"));
        assert!(takes_self("fn get(&'a mut self)"));
        assert!(!takes_self("pub fn new(folder: Folder) -> Self"));
    }

    #[test]
    fn a_doc_comment_reads_as_page_markdown() {
        let doc = "Reads [`Store`] from [`crate::a::B`](crate::a::B).\n\n# Examples\n\n```\n# use x::y;\nlet a = 1;\n```\n\n```text\n# kept\n```";
        assert_eq!(
            doc_markdown(doc),
            "Reads `Store` from `crate::a::B`.\n\n### Examples\n\n```rust\nlet a = 1;\n```\n\n```text\n# kept\n```"
        );
    }
}
