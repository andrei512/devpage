//! Navigation: the tree a document's pages form through their `## Topics` sections.
//!
//! A page's children are the links listed under its `## Topics` heading — the
//! first link of each list item, labelled with that link's text — in list order.
//! `### headings` inside Topics group them. The tree starts at the root page.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use comrak::nodes::{AstNode, NodeValue};
use comrak::{Arena, Options, parse_document};

use crate::document::Document;
use crate::html::Link;

/// One entry of the tree: a page and its own children. A page listed more
/// than once is an entry only at its position: its shallowest listing, the
/// first in reading order on a tie. Every other listing — the root page's
/// among them — is left out of the tree.
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

/// The children of the root page, recursively. Each page is in the tree only
/// at its position (see [`Entry`]); the root's position is the top, so cycles
/// end.
pub fn tree(document: &Document) -> Vec<Group> {
    let crate::document::Kind::Site { root } = &document.kind else {
        return Vec::new();
    };
    let root = document.base.join(root);
    let homes = homes(document, &root);
    children(document, &root, &homes)
}

/// Where each page listed in the tree has its position: the page listing it
/// and the listing's index among that page's Topics links. Found level by
/// level from the root, so the shallowest listing wins, then the first.
fn homes(document: &Document, root: &Path) -> HashMap<PathBuf, (PathBuf, usize)> {
    let mut homes = HashMap::new();
    let mut seen = HashSet::from([root.to_path_buf()]);
    let mut level = vec![root.to_path_buf()];
    while !level.is_empty() {
        let mut next = Vec::new();
        for page in &level {
            let Ok(source) = std::fs::read_to_string(page) else {
                continue;
            };
            for (index, topic) in topics(&source).into_iter().enumerate() {
                let file = match resolve(document, page, &topic.href) {
                    Target::Page(file) | Target::Document(file) => file,
                    Target::None | Target::Problem(_) => continue,
                };
                if seen.insert(file.clone()) {
                    homes.insert(file.clone(), (page.clone(), index));
                    next.push(file);
                }
            }
        }
        level = next;
    }
    homes
}

fn children(
    document: &Document,
    page: &Path,
    homes: &HashMap<PathBuf, (PathBuf, usize)>,
) -> Vec<Group> {
    let Ok(source) = std::fs::read_to_string(page) else {
        return Vec::new();
    };
    let mut groups: Vec<Group> = Vec::new();
    for (index, topic) in topics(&source).into_iter().enumerate() {
        let (file, page_target) = match resolve(document, page, &topic.href) {
            Target::Page(file) => (file, true),
            Target::Document(file) => (file, false),
            Target::None | Target::Problem(_) => continue,
        };
        let home = homes
            .get(&file)
            .is_some_and(|(parent, at)| parent == page && *at == index);
        if !home {
            continue;
        }
        let entry = Entry {
            label: topic.label,
            children: if page_target {
                children(document, &file, homes)
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

/// What a Topics link names in a site's sidebar.
#[derive(Debug, PartialEq)]
pub enum Target {
    /// A page in the site's folder: an entry with its own children.
    Page(PathBuf),
    /// A document file: an entry that opens that document, with no children.
    Document(PathBuf),
    /// Not an entry: another website, a `#fragment`, any other file.
    None,
    /// Left out, and a problem: a missing file, or a page outside the site.
    Problem(String),
}

/// What a Topics link on `page` names.
pub fn resolve(document: &Document, page: &Path, href: &str) -> Target {
    let path = match Link::classify(href) {
        Link::File { path, .. } if !path.is_empty() => path,
        _ => return Target::None,
    };
    let Some(target) = page
        .parent()
        .and_then(|folder| document.served.file(&folder.join(path)))
    else {
        return Target::Problem(format!("Topics entry {href} — no such file"));
    };
    if target.extension().is_some_and(|ext| ext == "md") {
        if target.starts_with(&document.base) {
            Target::Page(target)
        } else {
            Target::Problem(format!("Topics entry {href} — outside the site's folder"))
        }
    } else if crate::document::kind_of(&target).is_some() {
        Target::Document(target)
    } else {
        Target::None
    }
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
    use crate::folder::ServedFolder;

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
    fn a_page_listed_twice_is_in_the_tree_only_at_its_shallowest_listing() {
        let dir = std::env::temp_dir().join(format!("devpage-nav-home-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\n",
        )
        .unwrap();
        // Reading order meets `start` under `system` first, one level deeper.
        std::fs::write(
            dir.join("README.md"),
            "# Home\n\n## Topics\n\n- [System](system.md)\n- [Start](start.md)\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("system.md"),
            "# System\n\n## Topics\n\n- [Start](start.md)\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("start.md"),
            "# Start\n\n## Topics\n\n- [First](first.md)\n",
        )
        .unwrap();
        std::fs::write(dir.join("first.md"), "# First\n").unwrap();
        let document = Document::load(&dir, &ServedFolder::new(&dir).unwrap()).unwrap();

        let tree = tree(&document);
        let [system, start] = &tree[0].entries[..] else {
            panic!("two top-level entries");
        };
        assert_eq!(
            start.label, "Start",
            "the top-level listing is the position"
        );
        assert_eq!(start.children[0].entries[0].label, "First");
        assert!(system.children.is_empty(), "a second listing is left out");
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
            "# Home\n\n## Topics\n\n- [Guide](guide/README.md)\n- [Missing](nope.md)\n- [Talk](talk.yml)\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("talk.yml"),
            "kind: slides\ntitle: T\nslides: [README.md]\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("guide/README.md"),
            "# Guide\n\n## Topics\n\n- [Back home](../README.md)\n- [Step](step.md)\n",
        )
        .unwrap();
        std::fs::write(dir.join("guide/step.md"), "# Step\n").unwrap();
        let document = Document::load(&dir, &ServedFolder::new(&dir).unwrap()).unwrap();

        let tree = tree(&document);
        assert_eq!(tree.len(), 1);
        let talk = &tree[0].entries[1];
        assert_eq!(talk.label, "Talk", "a document file is an entry");
        assert!(talk.children.is_empty());
        let guide = &tree[0].entries[0];
        assert_eq!(guide.label, "Guide");
        let under_guide: Vec<_> = guide.children[0]
            .entries
            .iter()
            .map(|e| e.label.as_str())
            .collect();
        assert_eq!(
            under_guide,
            vec!["Step"],
            "a link back to the root is left out"
        );
    }

    #[test]
    fn a_page_outside_the_site_or_a_missing_one_is_a_problem() {
        let dir = std::env::temp_dir().join(format!("devpage-nav-out-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("site")).unwrap();
        let dir = dir.canonicalize().unwrap();
        std::fs::write(dir.join("outside.md"), "# Out\n").unwrap();
        std::fs::write(dir.join("site/README.md"), "# Home\n").unwrap();
        std::fs::write(
            dir.join("site/devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\n",
        )
        .unwrap();
        let document =
            Document::load(&dir.join("site"), &ServedFolder::new(&dir).unwrap()).unwrap();
        let page = dir.join("site/README.md");
        assert!(matches!(
            resolve(&document, &page, "../outside.md"),
            Target::Problem(message) if message.contains("outside the site's folder")
        ));
        assert!(matches!(
            resolve(&document, &page, "nope.md"),
            Target::Problem(_)
        ));
        assert_eq!(
            resolve(&document, &page, "https://example.com"),
            Target::None
        );
        assert_eq!(resolve(&document, &page, "#top"), Target::None);
    }
}
