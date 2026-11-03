//! Render: a reference page as Markdown, laid out as Apple lays out a
//! symbol page, which the page pipeline then shows in its site's frame like
//! any page of the site. A page opens with a hero — the kind, the name, the
//! abstract — then the declaration with the file it is in, the rest of the
//! doc, and what it holds, grouped by kind, each a declaration with its
//! abstract. No label is shown that the layout already says.

use std::path::Path;

use super::{Index, Member, Prose, Target, Targets};
use crate::html::escape;

/// The groups of a module's page, in order: a kind and its heading.
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
    let mut out = hero("Reference", &title_case(name), "");
    let crates: Vec<&Target> = targets
        .targets
        .values()
        .filter(|target| target.path.len() == 1)
        .collect();
    if crates.is_empty() {
        out.push_str("No code matches this namespace's `files`.\n");
        return out;
    }
    let mut groups = String::from("### Crates\n\n");
    for krate in crates {
        groups.push_str(&topic(
            &krate.path[0],
            "",
            &krate.path[0],
            &index.address(name, &krate.path),
            &krate.doc,
        ));
    }
    out.push_str(&topics(&groups));
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
    let (summary, overview) = split_doc(&target.doc);
    let mut out = hero(kind, &name, &summary);
    if target.kind != "module" && !target.signature.is_empty() {
        out.push_str(&declaration(
            index,
            &target.signature,
            &target.file,
            target.line,
        ));
    } else {
        out.push_str(&source(index, &target.file, None));
    }
    out.push_str(&overview_of(&overview, prose));

    let mut groups = String::new();
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
            groups.push_str(&format!("### {heading}\n\n"));
            for child in listed {
                let name = child.path.last().unwrap();
                let shown = if child.kind == "module" {
                    name.clone()
                } else {
                    short(&child.signature)
                };
                groups.push_str(&topic(
                    &shown,
                    if child.kind == "module" { "" } else { "rust" },
                    name,
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
        groups.push_str(&format!("### {heading}\n\n"));
        for member in members {
            let mut path = target.path.clone();
            path.push(member.name.clone());
            groups.push_str(&topic(
                &short(&member.signature),
                "rust",
                &member.name,
                &index.address(namespace, &path),
                &member.doc,
            ));
        }
    }
    if !target.implements.is_empty() {
        groups.push_str("### Implements\n\n");
        let chips: Vec<String> = target
            .implements
            .iter()
            .map(|name| format!("<code>{}</code>", escape(name)))
            .collect();
        groups.push_str(&format!(
            "<p class=\"code-chips\">{}</p>\n\n",
            chips.join(" ")
        ));
    }
    out.push_str(&topics(&groups));
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
    let (summary, overview) = split_doc(&member.doc);
    let kind = format!(
        "{} of <a href=\"{}\">{}</a>",
        group_of(member).label(),
        escape(&index.address(namespace, &owner.path)),
        escape(owner.path.last().map(String::as_str).unwrap_or(""))
    );
    let mut out = hero_html(&kind, &member.name, &summary);
    out.push_str(&declaration(
        index,
        &member.signature,
        &member.file,
        member.line,
    ));
    out.push_str(&overview_of(&overview, prose));
    out
}

/// What a member is, as a Topics group of its type's page.
#[derive(Clone, Copy, PartialEq)]
enum Group {
    /// A field of a struct.
    Field,
    /// A variant of an enum.
    Variant,
    /// A function of the type that takes no `self`.
    Function,
    /// A function of the type that takes `self`.
    Method,
}

impl Group {
    /// A member's kind as its page names it, before "of" and its type:
    /// `"Associated Function"`.
    fn label(self) -> &'static str {
        match self {
            Group::Field => "Field",
            Group::Variant => "Variant",
            Group::Function => "Associated Function",
            Group::Method => "Method",
        }
    }
}

/// The group `member` is listed under on its type's page.
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

/// The top of a page: its kind, its name and its abstract.
fn hero(kind: &str, name: &str, summary: &str) -> String {
    hero_html(&escape(kind), name, summary)
}

/// [`hero`], its kind already HTML.
fn hero_html(kind: &str, name: &str, summary: &str) -> String {
    let summary = if summary.is_empty() {
        String::new()
    } else {
        format!("{summary}\n\n")
    };
    format!(
        "<div class=\"code-hero\">\n\n<p class=\"code-eyebrow\">{kind}</p>\n\n# {name}\n\n{summary}</div>\n\n"
    )
}

/// The declaration as written, and under it the file and line it is in.
fn declaration(index: &Index, signature: &str, file: &Path, line: usize) -> String {
    format!(
        "<div class=\"code-declaration\">\n\n{}</div>\n\n{}",
        fence(signature),
        source(index, file, Some(line))
    )
}

/// The file — and line — something is declared at, linked as the served
/// folder serves it.
fn source(index: &Index, file: &Path, line: Option<usize>) -> String {
    match index.source(file) {
        Some((address, name)) => {
            let shown = match line {
                Some(line) => format!("{name}:{line}"),
                None => name,
            };
            format!(
                "<p class=\"code-source\"><a href=\"{}\">{}</a></p>\n\n",
                escape(&address),
                escape(&shown)
            )
        }
        None => String::new(),
    }
}

/// The rest of the doc, and the prose that adds to it.
fn overview_of(overview: &str, prose: Option<&Prose>) -> String {
    let prose = prose.map(|prose| prose.body.trim()).unwrap_or("");
    let mut out = String::new();
    for part in [overview, prose] {
        if !part.is_empty() {
            out.push_str(part);
            out.push_str("\n\n");
        }
    }
    if out.is_empty() {
        return out;
    }
    format!("<div class=\"code-overview\">\n\n{out}</div>\n\n")
}

/// What a page holds, its groups headed by kind.
fn topics(groups: &str) -> String {
    if groups.is_empty() {
        return String::new();
    }
    format!("<div class=\"code-topics\">\n\n{groups}</div>\n\n")
}

/// One entry of a group: `shown` — a declaration in `language`, or a bare
/// name — linked, its `name` in bold, and its abstract under it.
fn topic(shown: &str, language: &str, name: &str, address: &str, doc: &str) -> String {
    let (summary, _) = split_doc(doc);
    let summary = summary.split('\n').collect::<Vec<_>>().join(" ");
    let label = emphasized(shown, name);
    let class = if language.is_empty() {
        "code-topic-name"
    } else {
        "code-topic-declaration"
    };
    let mut out = format!(
        "<div class=\"code-topic\">\n\n<a class=\"{class}\" href=\"{}\"><code>{label}</code></a>\n\n",
        escape(address)
    );
    if !summary.is_empty() {
        out.push_str(&summary);
        out.push_str("\n\n");
    }
    out.push_str("</div>\n\n");
    out
}

/// `shown`, escaped, with the first `name` in it in bold.
fn emphasized(shown: &str, name: &str) -> String {
    let at = shown
        .match_indices(name)
        .find(|(at, _)| {
            let before = shown[..*at].chars().last();
            let after = shown[at + name.len()..].chars().next();
            !before.is_some_and(|c| c.is_alphanumeric() || c == '_')
                && !after.is_some_and(|c| c.is_alphanumeric() || c == '_')
        })
        .map(|(at, _)| at);
    match at {
        Some(at) => format!(
            "{}<b>{}</b>{}",
            escape(&shown[..at]),
            escape(name),
            escape(&shown[at + name.len()..])
        ),
        None => escape(shown),
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

/// How an item's `kind` is named on its page: `"struct"` is `"Structure"`; a
/// kind not known is shown as it is.
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

/// `name` with its first letter in upper case.
pub(super) fn title_case(name: &str) -> String {
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

/// The length of the longest run of `c` in `text`.
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
    fn a_topic_shows_its_name_in_bold() {
        assert_eq!(
            emphasized("fn read(&self, reader: R) -> u8", "read"),
            "fn <b>read</b>(&amp;self, reader: R) -&gt; u8"
        );
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
