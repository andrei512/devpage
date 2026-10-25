//! Navigation: the tree a document's pages form through their `## Topics` sections.
//!
//! A page's children are the links listed under its `## Topics` heading — the
//! first link of each list item, labelled with that link's text — in list order.
//! `### headings` inside Topics group them. The tree starts at the root page.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use comrak::nodes::{AstNode, NodeValue};
use comrak::{Arena, Options, parse_document};

use crate::document::Document;

/// One entry of the tree: a page and its own children.
#[derive(Debug, PartialEq)]
pub struct Entry {
    pub label: String,
    pub file: PathBuf,
    pub children: Vec<Group>,
}

/// Children under one `###` heading of a Topics section; `title` is `None` for
/// links listed before any group heading.
#[derive(Debug, PartialEq)]
pub struct Group {
    pub title: Option<String>,
    pub entries: Vec<Entry>,
}

/// A link listed in a Topics section, before it is resolved to a file.
#[derive(Debug, PartialEq)]
pub struct Topic {
    pub group: Option<String>,
    pub label: String,
    pub href: String,
}

/// The children of the root page, recursively. A page reached a second time is
/// listed again but not expanded, so cycles end.
pub fn tree(document: &Document) -> Vec<Group> {
    let crate::document::Kind::Site { root } = &document.kind else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    let root = document.base.join(root);
    seen.insert(root.clone());
    children(document, &root, &mut seen)
}

fn children(document: &Document, page: &Path, seen: &mut HashSet<PathBuf>) -> Vec<Group> {
    let Ok(source) = std::fs::read_to_string(page) else {
        return Vec::new();
    };
    let mut groups: Vec<Group> = Vec::new();
    for topic in topics(&source) {
        let Some(file) = resolve(document, page, &topic.href) else {
            continue;
        };
        let expand = seen.insert(file.clone());
        let entry = Entry {
            label: topic.label,
            children: if expand {
                children(document, &file, seen)
            } else {
                Vec::new()
            },
            file,
        };
        match groups.last_mut() {
            Some(group) if group.title == topic.group => group.entries.push(entry),
            _ => groups.push(Group {
                title: topic.group,
                entries: vec![entry],
            }),
        }
    }
    groups
}

/// A Topics link's target, when it is a Markdown file inside the document.
fn resolve(document: &Document, page: &Path, href: &str) -> Option<PathBuf> {
    if href.contains("://") || href.starts_with('#') {
        return None;
    }
    let path = href.split('#').next().unwrap_or(href);
    let target = page.parent()?.join(path).canonicalize().ok()?;
    (target.starts_with(&document.base)
        && target.extension().is_some_and(|ext| ext == "md")
        && target.is_file())
    .then_some(target)
}

/// The links listed in a page's `## Topics` section, in order.
pub fn topics(source: &str) -> Vec<Topic> {
    let arena = Arena::new();
    let root = parse_document(&arena, source, &Options::default());
    let mut topics = Vec::new();
    let mut in_topics = false;
    let mut group = None;
    for node in root.children() {
        let value = node.data.borrow().value.clone();
        match value {
            NodeValue::Heading(heading) if heading.level <= 2 => {
                in_topics = heading.level == 2 && text(node).trim() == "Topics";
                group = None;
            }
            NodeValue::Heading(heading) if in_topics && heading.level == 3 => {
                group = Some(text(node).trim().to_string());
            }
            NodeValue::List(_) if in_topics => {
                for item in node.children() {
                    if let Some((label, href)) = first_link(item) {
                        topics.push(Topic {
                            group: group.clone(),
                            label,
                            href,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    topics
}

fn first_link<'a>(node: &'a AstNode<'a>) -> Option<(String, String)> {
    node.descendants()
        .find_map(|descendant| match &descendant.data.borrow().value {
            NodeValue::Link(link) => Some((text(descendant).trim().to_string(), link.url.clone())),
            _ => None,
        })
}

fn text<'a>(node: &'a AstNode<'a>) -> String {
    node.descendants()
        .filter_map(|descendant| match &descendant.data.borrow().value {
            NodeValue::Text(text) => Some(text.to_string()),
            NodeValue::Code(code) => Some(code.literal.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_links_and_groups_from_the_topics_section_only() {
        let source = "# Home\n\nSee the [viewer](viewer.md) in passing.\n\n## Topics\n\n\
                      - [Vision](vision.md) — why\n\n### The system\n\n- [Viewer](boxes/viewer/README.md)\n\
                      - [Engineering](boxes/engineering/README.md)\n\n## Afterwards\n\n- [Ignored](x.md)\n";
        let found = topics(source);
        let labels: Vec<_> = found
            .iter()
            .map(|t| (t.group.as_deref(), t.label.as_str(), t.href.as_str()))
            .collect();
        assert_eq!(
            labels,
            vec![
                (None, "Vision", "vision.md"),
                (Some("The system"), "Viewer", "boxes/viewer/README.md"),
                (
                    Some("The system"),
                    "Engineering",
                    "boxes/engineering/README.md"
                ),
            ]
        );
    }

    #[test]
    fn builds_the_tree_and_stops_at_cycles() {
        let dir = std::env::temp_dir().join(format!("devpage-nav-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("guide")).unwrap();
        std::fs::write(
            dir.join("devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("README.md"),
            "# Home\n\n## Topics\n\n- [Guide](guide/README.md)\n- [Missing](nope.md)\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("guide/README.md"),
            "# Guide\n\n## Topics\n\n- [Back home](../README.md)\n- [Step](step.md)\n",
        )
        .unwrap();
        std::fs::write(dir.join("guide/step.md"), "# Step\n").unwrap();
        let document = Document::load(&dir).unwrap();

        let tree = tree(&document);
        assert_eq!(tree.len(), 1);
        let guide = &tree[0].entries[0];
        assert_eq!(guide.label, "Guide");
        let under_guide: Vec<_> = guide.children[0]
            .entries
            .iter()
            .map(|e| e.label.as_str())
            .collect();
        assert_eq!(under_guide, vec!["Back home", "Step"]);
        assert!(
            guide.children[0].entries[0].children.is_empty(),
            "the root is not expanded again"
        );
    }
}
