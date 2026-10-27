//! What is published, read for search: the pages the nav reaches and the
//! chapters and slides of the documents they reach, each split at its
//! headings into sections of prose and code, with diagram labels and
//! widget text taken as words.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use comrak::nodes::{AstNode, NodeValue};
use comrak::{Anchorizer, Arena, parse_document};
use ignore::WalkBuilder;

use crate::document::{Document, Kind};
use crate::index::Index;

use super::*;

/// A page to index, as the scope found it.
pub(super) struct Unit {
    pub(super) file: PathBuf,
    pub(super) kind: PageKind,
    pub(super) trail: Vec<String>,
    pub(super) document: Owner,
    /// The address of what holds it: the page itself, or the doc or deck.
    pub(super) address: String,
    /// A chapter's name, when its doc has more than one; a slide's number.
    pub(super) name: Option<String>,
}

/// One section of a Markdown file: its heading's text, id and level (none
/// and 0 for what comes before the first heading), and its text by kind.
#[derive(Debug, PartialEq, Default)]
pub(super) struct Section {
    pub(super) heading: String,
    pub(super) id: Option<String>,
    pub(super) level: u8,
    pub(super) prose: String,
    pub(super) code: String,
}

/// A Markdown file read for search: its sections and its first paragraph.
pub(super) struct Read {
    pub(super) sections: Vec<Section>,
    pub(super) lede: String,
}

/// A page's front matter, as search reads it: whether it is searched, and
/// its `description` and `keywords`.
#[derive(Default)]
pub(super) struct FrontMatter {
    pub(super) search: Option<bool>,
    pub(super) meta: String,
}

impl FrontMatter {
    pub(super) fn read(source: &str) -> Self {
        let Some(yaml) = crate::render::front_matter(source) else {
            return Self::default();
        };
        let Ok(serde_yaml::Value::Mapping(map)) = serde_yaml::from_str(yaml) else {
            return Self::default();
        };
        let text = |key: &str| match map.get(key) {
            Some(serde_yaml::Value::String(text)) => text.clone(),
            Some(serde_yaml::Value::Sequence(items)) => items
                .iter()
                .filter_map(|item| item.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            _ => String::new(),
        };
        Self {
            search: map.get("search").and_then(serde_yaml::Value::as_bool),
            meta: squeeze(&format!("{} {}", text("description"), text("keywords"))),
        }
    }
}

/// What is published from the document file `entry`: the pages its nav
/// reaches — its Topics tree, then its navbar — and the chapters and slides
/// of every document those reach, in reading order. A file that is both a
/// page and a chapter or slide is a chapter or slide only. A page out of
/// reach whose front matter says `search: true` comes last.
pub(super) fn scope(index: &Index, entry: &Path) -> Vec<Unit> {
    let mut walk = Walk {
        index,
        documents: BTreeSet::new(),
        units: Vec::new(),
    };
    walk.document(entry);
    let parts: BTreeSet<PathBuf> = walk
        .units
        .iter()
        .filter(|unit| unit.kind != PageKind::Page)
        .map(|unit| unit.file.clone())
        .collect();
    let mut seen = BTreeSet::new();
    let mut units: Vec<Unit> = walk
        .units
        .into_iter()
        .filter(|unit| unit.kind != PageKind::Page || !parts.contains(&unit.file))
        .filter(|unit| unit.kind != PageKind::Page || seen.insert(unit.file.clone()))
        .collect();
    for file in markdown_files(index, index.served().path()) {
        if seen.contains(&file) || parts.contains(&file) {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&file) else {
            continue;
        };
        if FrontMatter::read(&source).search != Some(true) {
            continue;
        }
        let site = index
            .site_for(&file)
            .and_then(|site| Document::load(&site, index.served()).ok());
        let address = index.served().url(&file);
        let document = match site {
            Some(site) => Owner {
                title: site.title.clone(),
                path: site.url(&site.path()),
            },
            None => Owner {
                title: crate::markup::title(&source, &file),
                path: address.clone(),
            },
        };
        units.push(Unit {
            file,
            kind: PageKind::Page,
            trail: Vec::new(),
            document,
            address,
            name: None,
        });
    }
    units
}

/// The walk from the served document through what it reaches.
pub(super) struct Walk<'a> {
    pub(super) index: &'a Index,
    pub(super) documents: BTreeSet<PathBuf>,
    pub(super) units: Vec<Unit>,
}

impl Walk<'_> {
    pub(super) fn in_sight(&self, file: &Path) -> bool {
        !self.index.ignored(file, false)
    }

    pub(super) fn document(&mut self, file: &Path) {
        if !self.documents.insert(file.to_path_buf()) || !self.in_sight(file) {
            return;
        }
        let Ok(document) = Document::load(file, self.index.served()) else {
            return;
        };
        let owner = Owner {
            title: document.title.clone(),
            path: document.url(&document.path()),
        };
        match &document.kind {
            Kind::Site { root } => {
                let trails = trails(&document);
                let page = |walk: &mut Self, file: &Path| {
                    if walk.in_sight(file) {
                        walk.units.push(Unit {
                            file: file.to_path_buf(),
                            kind: PageKind::Page,
                            trail: trails.get(file).cloned().unwrap_or_default(),
                            document: owner.clone(),
                            address: document.url(file),
                            name: None,
                        });
                    }
                };
                page(self, &document.base.join(root));
                fn listed(groups: &[crate::nav::Group], out: &mut Vec<PathBuf>) {
                    for entry in groups.iter().flat_map(|group| &group.entries) {
                        out.push(entry.file.clone());
                        listed(&entry.children, out);
                    }
                }
                let mut files = Vec::new();
                listed(&crate::nav::tree(&document), &mut files);
                for file in files {
                    if file.extension().is_some_and(|ext| ext == "md") {
                        page(self, &file);
                    } else {
                        self.document(&file);
                    }
                }
                for file in crate::navbar::reached(&document).0 {
                    if file.extension().is_some_and(|ext| ext == "md") {
                        if file.starts_with(&document.base) {
                            page(self, &file);
                        }
                    } else if crate::document::kind_of(&file).is_some() {
                        self.document(&file);
                    }
                }
            }
            Kind::Doc { chapters, .. } => {
                let scoped = chapters.len() > 1;
                for chapter in chapters {
                    if !self.in_sight(chapter) {
                        continue;
                    }
                    let name = chapter
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    let trail = if scoped {
                        std::fs::read_to_string(chapter)
                            .map(|source| vec![crate::markup::title(&source, chapter)])
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    self.units.push(Unit {
                        file: chapter.clone(),
                        kind: PageKind::Chapter,
                        trail,
                        document: owner.clone(),
                        address: owner.path.clone(),
                        name: scoped.then_some(name),
                    });
                }
            }
            Kind::Slides { slides, .. } => {
                for (n, slide) in slides.iter().enumerate() {
                    if !self.in_sight(&slide.file) {
                        continue;
                    }
                    self.units.push(Unit {
                        file: slide.file.clone(),
                        kind: PageKind::Slide,
                        trail: Vec::new(),
                        document: owner.clone(),
                        address: owner.path.clone(),
                        name: Some((n + 1).to_string()),
                    });
                }
            }
            Kind::Design => {}
        }
    }
}

/// Every in-sight Markdown file under `folder`. The walk honors the
/// `.gitignore` files under `folder`, as the index's does, so it never
/// descends into a build folder; the index judges what those above it say.
pub(super) fn markdown_files(index: &Index, folder: &Path) -> Vec<PathBuf> {
    let walk = WalkBuilder::new(folder)
        .hidden(false)
        .parents(false)
        .ignore(false)
        .git_global(false)
        .git_exclude(false)
        .require_git(false)
        .filter_entry(|entry| entry.file_name() != ".git" && entry.file_name() != ".devpage")
        .build();
    let mut files: Vec<PathBuf> = walk
        .flatten()
        .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
        .map(|entry| entry.into_path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .filter(|path| !index.ignored(path, false))
        .collect();
    files.sort();
    files
}

/// Each page in a site's nav, with the labels from the top of the tree down
/// to it; a page listed twice has the trail to its position.
pub(super) fn trails(document: &Document) -> BTreeMap<PathBuf, Vec<String>> {
    pub(super) fn walk(
        groups: &[crate::nav::Group],
        above: &[String],
        out: &mut BTreeMap<PathBuf, Vec<String>>,
    ) {
        for entry in groups.iter().flat_map(|group| &group.entries) {
            let mut trail = above.to_vec();
            trail.push(entry.label.clone());
            walk(&entry.children, &trail, out);
            if entry.home {
                out.entry(entry.file.clone()).or_insert(trail);
            }
        }
    }
    let mut out = BTreeMap::new();
    if let Kind::Site { root } = &document.kind {
        out.insert(document.base.join(root), Vec::new());
    }
    walk(&crate::nav::tree(document), &[], &mut out);
    out
}

/// A slide's title: its first `# ` heading, else its first `title="…"`
/// attribute — a `<Slide>`'s or a `<Hero>`'s — else its number.
pub(super) fn slide_title(source: &str, read: &Read, number: &str) -> String {
    if let Some(section) = read.sections.iter().find(|s| s.level == 1) {
        return section.heading.clone();
    }
    source
        .split("title=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .map(str::to_string)
        .unwrap_or_else(|| format!("Slide {number}"))
}

/// A Markdown file split at its headings, each heading with the id the
/// rendered page gives it (`render::markdown`), and its text sorted by kind:
/// prose — with a diagram's labels and a widget's text — and code.
pub(super) fn read(source: &str) -> Read {
    let arena = Arena::new();
    let root = parse_document(&arena, source, &crate::render::options());
    let lede = root
        .children()
        .find(|node| matches!(node.data().value, NodeValue::Paragraph))
        .map(|node| squeeze(&node.collect_text()))
        .unwrap_or_default();
    let mut anchorizer = Anchorizer::new();
    let mut sections = vec![Section::default()];
    let mut stack: Vec<&AstNode> = root.reverse_children().collect();
    while let Some(node) = stack.pop() {
        if let NodeValue::Heading(heading) = node.data().value {
            let text = node.collect_text();
            let mut id = anchorizer.anchorize(&text);
            if crate::render::FRAME_IDS.contains(&id.as_str()) {
                id.push_str("-heading");
            }
            sections.push(Section {
                heading: text.trim().to_string(),
                id: Some(id),
                level: heading.level,
                ..Section::default()
            });
            continue;
        }
        let current = sections.last_mut().expect("there is always a section");
        match &node.data().value {
            NodeValue::Text(text) => current.prose.push_str(text),
            // Inline code reads as part of its sentence, and is code too.
            NodeValue::Code(code) => {
                current.prose.push_str(&code.literal);
                current.code.push(' ');
                current.code.push_str(&code.literal);
            }
            NodeValue::CodeBlock(block) => {
                if block.info.split_whitespace().next() == Some("mermaid") {
                    current.prose.push(' ');
                    current.prose.push_str(&diagram_labels(&block.literal));
                    current.prose.push(' ');
                } else {
                    current.code.push(' ');
                    current.code.push_str(&block.literal);
                }
            }
            // A slide's `<Notes>` are not shown on the page; a deck adds them
            // to its slide's text itself.
            NodeValue::HtmlBlock(block) if !block.literal.trim_start().starts_with("<Notes") => {
                current.prose.push(' ');
                current.prose.push_str(&widget_text(&block.literal));
                current.prose.push(' ');
            }
            NodeValue::SoftBreak | NodeValue::LineBreak => current.prose.push(' '),
            NodeValue::HtmlBlock(_) | NodeValue::HtmlInline(_) | NodeValue::FrontMatter(_) => {}
            _ => {
                if node.data().value.block() {
                    current.prose.push(' ');
                }
                stack.extend(node.reverse_children());
            }
        }
    }
    for section in &mut sections {
        section.prose = squeeze(&section.prose);
        section.code = squeeze(&section.code);
    }
    // What comes before the first heading is a section only if it has text.
    if sections[0].prose.is_empty() && sections[0].code.is_empty() {
        sections.remove(0);
    }
    Read { sections, lede }
}

/// `text` with every run of whitespace one space, and none at the ends.
pub(super) fn squeeze(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A Mermaid diagram's labels — what its nodes, edges and messages say —
/// without its source: no ids, arrows, styles or `click` lines.
pub(super) fn diagram_labels(source: &str) -> String {
    const SKIPPED: &[&str] = &[
        "click",
        "classDef",
        "class",
        "style",
        "linkStyle",
        "flowchart",
        "graph",
        "sequenceDiagram",
        "stateDiagram",
        "stateDiagram-v2",
        "classDiagram",
        "erDiagram",
        "direction",
        "end",
        "autonumber",
    ];
    let mut labels = Vec::new();
    for line in source.lines() {
        let line = line.trim();
        let first = line.split_whitespace().next().unwrap_or("");
        if line.is_empty() || line.starts_with("%%") || SKIPPED.contains(&first) {
            continue;
        }
        if matches!(first, "participant" | "actor") {
            let name = line
                .split_once(" as ")
                .map_or(&line[first.len()..], |(_, as_)| as_);
            labels.push(name.trim().to_string());
            continue;
        }
        // A message, or a note: the text after its colon.
        if (line.contains("->") || first == "Note" || first == "note")
            && let Some((_, text)) = line.split_once(':')
        {
            labels.push(text.trim().to_string());
            continue;
        }
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let close = match chars[i] {
                '[' => ']',
                '(' => ')',
                '{' => '}',
                '|' => '|',
                '"' => '"',
                _ => {
                    i += 1;
                    continue;
                }
            };
            let open = chars[i];
            let mut depth = 1;
            let mut j = i + 1;
            while j < chars.len() {
                if chars[j] == close && (open == close || depth == 1) {
                    break;
                }
                if open != close && chars[j] == open {
                    depth += 1;
                } else if open != close && chars[j] == close {
                    depth -= 1;
                }
                j += 1;
            }
            let label: String = chars[i + 1..j.min(chars.len())].iter().collect();
            labels.push(label);
            i = j + 1;
        }
    }
    let text = labels
        .iter()
        .map(|label| {
            strip_tags(label)
                .trim_matches(|c: char| "\"'()[]{}/\\ ".contains(c))
                .to_string()
        })
        .filter(|label| !label.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    squeeze(&text)
}

/// The attributes of a widget whose values are its own, not text.
pub(super) const NOT_TEXT: &[&str] = &[
    "href",
    "src",
    "to",
    "class",
    "className",
    "id",
    "style",
    "icon",
    "logo",
    "width",
    "height",
    "size",
    "columns",
    "kind",
    "variant",
    "type",
    "lang",
    "language",
    "delivery",
    "target",
    "rel",
];

/// The text of an HTML block: what is between its tags, and its tags' text
/// attributes — never its markup, an expression `{…}`, or an address.
pub(super) fn widget_text(html: &str) -> String {
    let chars: Vec<char> = html.chars().collect();
    let mut text = String::new();
    let mut i = 0;
    let starts = |i: usize, s: &str| chars[i..].iter().take(s.len()).copied().eq(s.chars());
    while i < chars.len() {
        if starts(i, "<!--") {
            i = (i..chars.len())
                .find(|&j| starts(j, "-->"))
                .map_or(chars.len(), |j| j + 3);
            continue;
        }
        let tag = chars[i] == '<'
            && chars
                .get(i + 1)
                .is_some_and(|c| c.is_alphabetic() || *c == '/' || *c == '!');
        if !tag {
            text.push(chars[i]);
            i += 1;
            continue;
        }
        // Inside a tag: its name, then its attributes, up to its `>`.
        i += 1;
        while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '>' && chars[i] != '/' {
            i += 1;
        }
        let mut name = String::new();
        while i < chars.len() && chars[i] != '>' {
            let c = chars[i];
            if c.is_alphanumeric() || c == '-' || c == '_' {
                name.push(c);
                i += 1;
                continue;
            }
            if c == '=' {
                i += 1;
                continue;
            }
            if c == '"' || c == '\'' {
                let end = (i + 1..chars.len())
                    .find(|&j| chars[j] == c)
                    .unwrap_or(chars.len());
                let value: String = chars[i + 1..end].iter().collect();
                if !NOT_TEXT.contains(&name.as_str()) && !value.contains("://") {
                    text.push(' ');
                    text.push_str(&value);
                    text.push(' ');
                }
                name.clear();
                i = end + 1;
                continue;
            }
            if c == '{' {
                let mut depth = 0;
                while i < chars.len() {
                    match chars[i] {
                        '{' => depth += 1,
                        '}' => depth -= 1,
                        _ => {}
                    }
                    i += 1;
                    if depth == 0 {
                        break;
                    }
                }
                name.clear();
                continue;
            }
            name.clear();
            i += 1;
        }
        text.push(' ');
        i += 1;
    }
    squeeze(&text)
}

pub(super) fn strip_tags(html: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                text.push(' ');
            }
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    text
}
