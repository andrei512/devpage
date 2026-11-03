//! Render: a reference page as Markdown, which the page pipeline then shows
//! in its site's frame like any page of the site. A namespace's page lists
//! its crates; a module's lists its items by kind; a type's shows its
//! declaration, its doc, the prose that adds to it, and a section for each
//! field, variant and method.

use super::{Index, Member, Prose, Target, Targets};

/// The heading a page gives each kind of item listed on a module's page,
/// in the order they are listed.
const GROUPS: [(&str, &str); 10] = [
    ("module", "Modules"),
    ("struct", "Structs"),
    ("enum", "Enums"),
    ("union", "Unions"),
    ("trait", "Traits"),
    ("type", "Type aliases"),
    ("function", "Functions"),
    ("constant", "Constants"),
    ("static", "Statics"),
    ("macro", "Macros"),
];

/// The page of a namespace: every crate it holds.
pub fn namespace(index: &Index, name: &str, targets: &Targets) -> String {
    let mut out = format!("# {} reference\n\n", title_case(name));
    let crates: Vec<&Target> = targets
        .targets
        .values()
        .filter(|target| target.path.len() == 1)
        .collect();
    if crates.is_empty() {
        out.push_str("No code matches this namespace's `files`.\n");
    }
    for krate in crates {
        out.push_str(&format!(
            "- [`{}`]({}){}\n",
            krate.path[0],
            index.address(name, &krate.path, None),
            summary_suffix(&krate.doc)
        ));
    }
    out
}

/// The page of one target.
pub fn target(
    index: &Index,
    namespace: &str,
    targets: &Targets,
    target: &Target,
    prose: Option<&Prose>,
) -> String {
    let name = target.path.last().cloned().unwrap_or_default();
    let mut out = format!("# {name}\n\n");
    // Where it is: its kind, the module holding it, the file declaring it.
    let mut place = vec![kind_label(&target.kind).to_string()];
    if target.path.len() > 1 {
        let parent = &target.path[..target.path.len() - 1];
        place.push(format!(
            "in [`{}`]({})",
            parent.join("::"),
            index.address(namespace, parent, None)
        ));
    }
    if let Some(declared) = declared_in(index, &target.file, target.line) {
        place.push(format!("· {declared}"));
    }
    out.push_str(&format!("{}\n\n", place.join(" ")));
    if !target.signature.is_empty() && target.kind != "module" {
        out.push_str(&fence(&target.signature));
    }
    if !target.doc.is_empty() {
        out.push_str(&doc_markdown(&target.doc));
        out.push_str("\n\n");
    }
    if let Some(prose) = prose
        && !prose.body.trim().is_empty()
    {
        out.push_str(prose.body.trim());
        out.push_str("\n\n");
    }
    if target.kind == "module" {
        let children: Vec<&Target> = targets
            .targets
            .values()
            .filter(|child| {
                child.path.len() == target.path.len() + 1 && child.path.starts_with(&target.path)
            })
            .collect();
        for (kind, heading) in GROUPS {
            let listed: Vec<&&Target> = children.iter().filter(|c| c.kind == kind).collect();
            if listed.is_empty() {
                continue;
            }
            out.push_str(&format!("## {heading}\n\n"));
            for child in listed {
                out.push_str(&format!(
                    "- [`{}`]({}){}\n",
                    child.path.last().unwrap(),
                    index.address(namespace, &child.path, None),
                    summary_suffix(&child.doc)
                ));
            }
            out.push('\n');
        }
    }
    for (kind, heading) in [
        ("field", "Fields"),
        ("variant", "Variants"),
        ("method", "Methods"),
    ] {
        let members: Vec<&Member> = target.members.iter().filter(|m| m.kind == kind).collect();
        if members.is_empty() {
            continue;
        }
        out.push_str(&format!("## {heading}\n\n"));
        for member in members {
            out.push_str(&format!("### {}\n\n", member.name));
            out.push_str(&fence(&member.signature));
            if !member.doc.is_empty() {
                out.push_str(&doc_markdown(&member.doc));
                out.push_str("\n\n");
            }
            if member.file != target.file
                && let Some(declared) = declared_in(index, &member.file, member.line)
            {
                out.push_str(&format!("{declared}\n\n"));
            }
        }
    }
    if !target.implements.is_empty() {
        out.push_str("## Implements\n\n");
        let traits: Vec<String> = target
            .implements
            .iter()
            .map(|name| format!("`{name}`"))
            .collect();
        out.push_str(&traits.join(", "));
        out.push_str("\n\n");
    }
    out
}

/// The id a member's section heading gets: the one comrak gives `### name`.
pub fn member_id(name: &str) -> String {
    comrak::Anchorizer::new().anchorize(name)
}

/// "Declared in `file`, line n", linking the file as the served folder
/// serves it.
fn declared_in(index: &Index, file: &std::path::Path, line: usize) -> Option<String> {
    let (address, name) = index.source(file)?;
    Some(format!("Declared in [{name}]({address}), line {line}"))
}

fn kind_label(kind: &str) -> &str {
    match kind {
        "module" => "Module",
        "struct" => "Struct",
        "enum" => "Enum",
        "union" => "Union",
        "trait" => "Trait",
        "type" => "Type alias",
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

/// ` — ` and the first sentence of `doc`, for a list entry; nothing for an
/// undocumented item.
fn summary_suffix(doc: &str) -> String {
    let first: Vec<&str> = doc
        .lines()
        .take_while(|line| !line.trim().is_empty())
        .collect();
    let paragraph = first.join(" ");
    if paragraph.is_empty() || paragraph.starts_with('#') || paragraph.starts_with("```") {
        return String::new();
    }
    let sentence = match paragraph.find(". ") {
        Some(at) => &paragraph[..=at],
        None => paragraph.as_str(),
    };
    format!(" — {}", intra_doc_links(sentence.trim()))
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
    fn a_doc_comment_reads_as_page_markdown() {
        let doc = "Reads [`Store`] from [`crate::a::B`](crate::a::B).\n\n# Examples\n\n```\n# use x::y;\nlet a = 1;\n```\n\n```text\n# kept\n```";
        assert_eq!(
            doc_markdown(doc),
            "Reads `Store` from `crate::a::B`.\n\n### Examples\n\n```rust\nlet a = 1;\n```\n\n```text\n# kept\n```"
        );
    }

    #[test]
    fn a_list_entry_takes_the_docs_first_sentence() {
        assert_eq!(
            summary_suffix("Holds entries. More here.\nAnd here.\n\nLater."),
            " — Holds entries."
        );
        assert_eq!(summary_suffix(""), "");
    }

    #[test]
    fn a_member_id_is_the_id_its_heading_gets() {
        assert_eq!(member_id("from_str"), "from_str");
        assert_eq!(member_id("Read"), "read");
    }
}
