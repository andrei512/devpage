//! A site's structure: the page types its structure folder defines, and what
//! is wrong with a page against them — no type, a type the folder lacks, a
//! parent its type doesn't allow, no place in the tree, a missing section.
//! devpage has no page types of its own: every one is a type file's.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use comrak::nodes::NodeValue;
use comrak::{Arena, Options, parse_document};
use serde::Deserialize;

use crate::document::{Document, Kind};
use crate::markup::split_front_matter;
use crate::nav;
use crate::problem::Problem;

/// One name, or a list of them.
#[derive(Deserialize)]
#[serde(untagged)]
enum Names {
    /// One name.
    One(String),
    /// Several.
    Many(Vec<String>),
}

/// A type file's front matter, as written.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Rules {
    /// The types a page of this type may sit under.
    parent: Option<Names>,
    /// The `##` headings a page of this type has, in order.
    sections: Option<Vec<String>>,
}

/// One page type: what its type file asks of a page.
struct Type {
    /// The types a page of this type may sit under; `None` for any.
    parent: Option<Vec<String>>,
    /// The `##` headings a page of this type has, in order.
    sections: Vec<String>,
}

/// A site's page types and where each of its pages sits, read now.
pub struct Structure {
    /// The structure folder.
    folder: PathBuf,
    /// The folder as problems name it: from the top of the served folder.
    folder_name: String,
    /// The types, by name: each type file's name without `.md`.
    types: BTreeMap<String, Type>,
    /// The type files, in name order.
    files: Vec<PathBuf>,
    /// What is wrong with the type files themselves.
    problems: Vec<Problem>,
    /// The site's root page, which sits under none.
    root: PathBuf,
    /// The page each listed page sits under.
    parents: HashMap<PathBuf, PathBuf>,
}

impl Structure {
    /// `document`'s structure; `None` unless it is a site that names a
    /// structure folder.
    pub fn of(document: &Document) -> Option<Self> {
        let folder = document.structure.clone()?;
        let Kind::Site { root } = &document.kind else {
            return None;
        };
        let folder_name = document
            .name(&folder)
            .unwrap_or_else(|| folder.display().to_string());
        let mut files: Vec<PathBuf> = std::fs::read_dir(&folder)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|file| file.extension().is_some_and(|ext| ext == "md"))
            .collect();
        files.sort();
        let mut types = BTreeMap::new();
        let mut problems = Vec::new();
        let mut named = Vec::new();
        for file in &files {
            let name = file
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let shown = document
                .name(file)
                .unwrap_or_else(|| file.display().to_string());
            let source = std::fs::read_to_string(file).unwrap_or_default();
            let yaml = split_front_matter(&source).map_or("", |(yaml, _)| yaml);
            let rules = if yaml.trim().is_empty() {
                Rules::default()
            } else {
                serde_yaml::from_str(yaml).unwrap_or_else(|error: serde_yaml::Error| {
                    problems.push(Problem {
                        file: shown.clone(),
                        // The front matter starts on the file's second line.
                        line: error.location().map(|at| at.line() + 1),
                        column: None,
                        message: format!("front matter — {error}"),
                        target: None,
                    });
                    Rules::default()
                })
            };
            let parent = rules.parent.map(|names| match names {
                Names::One(name) => vec![name],
                Names::Many(names) => names,
            });
            for parent in parent.iter().flatten() {
                named.push((shown.clone(), line_of(&source, parent, 0), parent.clone()));
            }
            types.insert(
                name,
                Type {
                    parent,
                    sections: rules.sections.unwrap_or_default(),
                },
            );
        }
        for (file, line, parent) in named {
            if !types.contains_key(&parent) {
                problems.push(Problem {
                    file,
                    line,
                    column: None,
                    message: format!("parent {parent} — no {folder_name}/{parent}.md"),
                    target: None,
                });
            }
        }
        Some(Structure {
            folder,
            folder_name,
            types,
            files,
            problems,
            root: document.base.join(root),
            parents: nav::parents(document),
        })
    }

    /// The type files: what the structure is read from.
    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    /// What is wrong with the type files themselves.
    pub fn type_problems(&self) -> Vec<Problem> {
        self.problems.clone()
    }

    /// What is wrong with `page`, whose Markdown is `source`, as a page of
    /// `document`'s site: its own type, place and sections, and each page
    /// its Topics place under it whose type doesn't allow this parent — on
    /// this page, at the entry. A type file, the site's `404.md` and a page
    /// outside the site's folder have none.
    pub fn problems(&self, document: &Document, page: &Path, source: &str) -> Vec<Problem> {
        let Some(file) = document.name(page) else {
            return Vec::new();
        };
        if !page.starts_with(&document.base)
            || page.starts_with(&self.folder)
            || page == document.base.join("404.md")
        {
            return Vec::new();
        }
        let problem = |line: Option<usize>, message: String, target: Option<String>| Problem {
            file: file.clone(),
            line,
            column: None,
            message,
            target,
        };
        let mut problems = Vec::new();
        let own = page_type(source);
        match &own {
            None => problems.push(problem(None, "no page-type".to_string(), None)),
            Some((name, line)) if !self.types.contains_key(name) => problems.push(problem(
                Some(*line),
                format!("page-type {name} — no {}/{name}.md", self.folder_name),
                None,
            )),
            Some(_) => {}
        }
        if page != self.root && !self.parents.contains_key(page) {
            problems.push(problem(
                None,
                "no Topics section lists this page".to_string(),
                None,
            ));
        }
        let Some((name, own)) = own.and_then(|(name, _)| {
            let own = self.types.get(&name)?;
            Some((name, own))
        }) else {
            return problems;
        };
        for (line, message) in sections(source, &name, &own.sections) {
            problems.push(problem(line, message, None));
        }
        let mut placed = HashSet::new();
        for topic in nav::topics(source) {
            let nav::Target::Page(child) = nav::resolve(document, page, &topic.href) else {
                continue;
            };
            if self.parents.get(&child).map(PathBuf::as_path) != Some(page)
                || child.starts_with(&self.folder)
                || !placed.insert(child.clone())
            {
                continue;
            }
            let Some((child_type, _)) = std::fs::read_to_string(&child)
                .ok()
                .and_then(|source| page_type(&source))
            else {
                continue;
            };
            let Some(allowed) = self
                .types
                .get(&child_type)
                .and_then(|kind| kind.parent.as_ref())
            else {
                continue;
            };
            if !allowed.contains(&name) {
                let from = source.find("\n## Topics").unwrap_or(0);
                problems.push(problem(
                    line_of(source, &topic.href, from),
                    format!(
                        "{} — a page of type {child_type} sits under {}, and this page's type is {name}",
                        topic.href,
                        any_of(allowed)
                    ),
                    Some(topic.href.clone()),
                ));
            }
        }
        problems
    }
}

/// The line `needle` first stands on at or after byte `from` of `source`,
/// counted from 1.
fn line_of(source: &str, needle: &str, from: usize) -> Option<usize> {
    let at = from + source[from..].find(needle)?;
    Some(source[..at].lines().count().max(1))
}

/// `a`, `a or b`, `a, b or c`.
fn any_of(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [most @ .., last] => format!("{} or {last}", most.join(", ")),
    }
}

/// A page's `page-type` and the line it is written on; `None` when its front
/// matter names none.
pub fn page_type(source: &str) -> Option<(String, usize)> {
    let (yaml, _) = split_front_matter(source)?;
    let map: serde_yaml::Mapping = serde_yaml::from_str(yaml).ok()?;
    let name = map.get("page-type")?.as_str()?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let line = yaml
        .lines()
        .position(|line| line.starts_with("page-type"))
        .map_or(1, |at| at + 2);
    Some((name, line))
}

/// The text a page's front matter gives `key`, trimmed; `None` when it has no
/// front matter, no such key, or nothing written for it.
pub fn front_matter(source: &str, key: &str) -> Option<String> {
    let (yaml, _) = split_front_matter(source)?;
    let map: serde_yaml::Mapping = serde_yaml::from_str(yaml).ok()?;
    let value = map.get(key)?.as_str()?.trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// What is wrong with a page's sections against the `required` ones of its
/// type `name`: each one missing, and each one standing before one it follows.
fn sections(source: &str, name: &str, required: &[String]) -> Vec<(Option<usize>, String)> {
    if required.is_empty() {
        return Vec::new();
    }
    let body = split_front_matter(source).map_or(source, |(_, body)| body);
    // Lines are counted in the file, front matter and all.
    let before = source[..source.len() - body.len()].lines().count();
    let arena = Arena::new();
    let root = parse_document(&arena, body, &Options::default());
    let headings: Vec<(String, usize)> = root
        .children()
        .filter_map(|node| {
            let data = node.data.borrow();
            match &data.value {
                NodeValue::Heading(heading) if heading.level == 2 => Some((
                    nav::text(node).trim().to_string(),
                    before + data.sourcepos.start.line,
                )),
                _ => None,
            }
        })
        .collect();
    let has = required
        .iter()
        .map(|heading| format!("## {heading}"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut problems = Vec::new();
    let mut last: Option<(&str, usize)> = None;
    for heading in required {
        let Some((_, line)) = headings.iter().find(|(found, _)| found == heading) else {
            problems.push((
                None,
                format!("no ## {heading} section — a page of type {name} has {has}"),
            ));
            continue;
        };
        match last {
            Some((earlier, at)) if *line < at => problems.push((
                Some(*line),
                format!("## {heading} comes before ## {earlier} — a page of type {name} has {has}"),
            )),
            _ => last = Some((heading, *line)),
        }
    }
    problems
}
