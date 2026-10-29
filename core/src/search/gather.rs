//! What is published, read for search: the pages the nav reaches and the
//! chapters and slides of the documents they reach, each split at its
//! headings into sections of prose and code, with diagram labels and
//! widget text taken as words.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use comrak::nodes::{AstNode, NodeValue};
use comrak::{Arena, parse_document};
use ignore::WalkBuilder;

use crate::document::{Document, Kind};
use crate::folder::Folder;
use crate::page::Rendered;
use crate::widgets::{Modules, Widgets};

use super::*;

/// A page to index, as the scope found it: all it is besides its file's
/// text, so a page whose unit and file are as they were is indexed as it was.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Unit {
    pub(super) file: PathBuf,
    pub(super) kind: PageKind,
    pub(super) trail: Vec<String>,
    pub(super) document: Owner,
    /// The address of what holds it: the page itself, or the doc or deck.
    pub(super) address: String,
    /// A chapter's name, when its doc has more than one; a slide's number.
    pub(super) name: Option<String>,
    /// Its place in the nav, counted from the top in reading order.
    pub(super) nav: u64,
    /// What its content is rendered by, as its page renders it: the
    /// document file whose widgets its tags are expanded with — its site,
    /// its doc or deck, or the page alone — and that document's widgets
    /// folder. `None` when that document can't be read, and its page shows
    /// none of the file.
    pub(super) shown_in: Option<Shown>,
}

/// The document a page's content is rendered by, and its widgets folder.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Shown {
    pub(super) file: PathBuf,
    pub(super) widgets: Option<PathBuf>,
}

impl Shown {
    fn of(document: &Document) -> Self {
        Self {
            file: document.path(),
            widgets: document.widgets.clone(),
        }
    }
}

/// Each document file loaded, `None` when it can't be.
pub(super) type Documents = HashMap<PathBuf, Option<Arc<Document>>>;

#[cfg(test)]
thread_local! {
    /// Every Markdown file read on this thread to find what is published,
    /// for the tests: how many pages a pass walks.
    pub(super) static WALKED: std::cell::RefCell<Vec<PathBuf>> = Default::default();
}

/// A Markdown file's text, read to find what is published; `None` when it
/// can't be read.
fn markdown(file: &Path) -> Option<String> {
    #[cfg(test)]
    WALKED.with_borrow_mut(|walked| walked.push(file.to_path_buf()));
    std::fs::read_to_string(file).ok()
}

/// What is published from the entry, and what finding it read: every page
/// published, by address; the documents loaded; and the footprint of every
/// Markdown file in sight. Kept from one pass to the next, so a save that
/// leaves every footprint and document file as it was leaves it as it was.
pub(super) struct Published {
    pub(super) units: BTreeMap<String, Unit>,
    pub(super) documents: Documents,
    footprints: HashMap<PathBuf, Footprint>,
}

/// What of a Markdown file decides what is published, as far as it does
/// where it is: its Topics, when a site's nav reads them; the trail it
/// gives its chapter, when it is one of a doc's chapters; and, when the
/// walk reaches it nowhere, whether its front matter says it is searched
/// and, out of every site, the title it names its own document by. The
/// rest of the file is its content, which only its page shows.
#[derive(Debug, Default, PartialEq)]
struct Footprint {
    topics: Option<Vec<crate::nav::Topic>>,
    trail: Option<Vec<String>>,
    search: Option<Option<bool>>,
    title: Option<String>,
}

impl Footprint {
    /// The parts of `source` that `was` holds, read again.
    fn again(was: &Self, source: &str, file: &Path) -> Self {
        Self {
            topics: was.topics.as_ref().map(|_| crate::nav::topics(source)),
            trail: was
                .trail
                .as_ref()
                .map(|_| vec![crate::markup::title(source, file)]),
            search: was
                .search
                .as_ref()
                .map(|_| FrontMatter::read(source).search),
            title: was
                .title
                .as_ref()
                .map(|_| crate::markup::title(source, file)),
        }
    }
}

impl Published {
    /// Finds what the document file `entry` publishes ([`scope`]).
    pub(super) fn find(pass: &mut Pass, entry: &Path) -> Self {
        let (units, footprints) = scope(pass, entry);
        Self {
            units: units.into_iter().map(|unit| (unit.url(), unit)).collect(),
            documents: HashMap::new(),
            footprints,
        }
    }

    /// Whether a change to `paths` can change what is published: a folder
    /// or a path gone, a `.gitignore`, a YAML file — a document file, or
    /// one that may have become one — or a Markdown file new to it or whose
    /// footprint differs. A Markdown file whose footprint is as it was, or
    /// a widget's `.tsx`, changes only what pages show.
    pub(super) fn moved_by(&self, paths: &[PathBuf]) -> bool {
        paths.iter().any(|path| {
            if path.is_dir()
                || !path.exists()
                || path.file_name().is_some_and(|name| name == ".gitignore")
                || path
                    .extension()
                    .is_some_and(|ext| ext == "yml" || ext == "yaml")
            {
                return true;
            }
            if !path.extension().is_some_and(|ext| ext == "md") {
                return false;
            }
            let Some(was) = self.footprints.get(path) else {
                return true;
            };
            // A page whose place no part of it decides is not read at all.
            *was != Footprint::default()
                && markdown(path).is_none_or(|source| Footprint::again(was, &source, path) != *was)
        })
    }
}

/// What one pass over what is published reads its documents through: each
/// document file loaded once, however many of its pages the pass meets,
/// and each document's widgets read once, only when a page it shows is read.
pub(super) struct Pass<'a> {
    pub(super) folder: &'a Folder,
    /// Each document file loaded, `None` when it can't be.
    documents: Documents,
    /// Each document's widgets, by its file.
    widgets: HashMap<PathBuf, Option<Widgets>>,
    modules: Modules,
}

impl<'a> Pass<'a> {
    pub(super) fn new(folder: &'a Folder) -> Self {
        Self::with(folder, HashMap::new())
    }

    /// A pass that starts from `documents`, the documents an earlier pass
    /// loaded: what is published was found with them, and is unchanged.
    pub(super) fn with(folder: &'a Folder, documents: Documents) -> Self {
        Self {
            folder,
            documents,
            widgets: HashMap::new(),
            modules: Modules::default(),
        }
    }

    /// The documents the pass loaded, to start the next one from.
    pub(super) fn into_documents(self) -> Documents {
        self.documents
    }

    /// The document `file` is — `devpage.yml` in a folder — as the pass
    /// first loaded it.
    pub(super) fn document(&mut self, file: &Path) -> Option<Arc<Document>> {
        let file = if file.is_dir() {
            file.join("devpage.yml")
        } else {
            file.to_path_buf()
        };
        let served = self.folder.served();
        self.documents
            .entry(file)
            .or_insert_with_key(|file| Document::load(file, served).ok().map(Arc::new))
            .clone()
    }

    /// The document a Markdown file is shown in at its own address
    /// ([`crate::page::shown_in`]).
    fn shown(&mut self, file: &Path) -> Option<Shown> {
        let by = crate::page::shown_by(file, self.folder);
        self.document(&by).map(|document| Shown::of(&document))
    }

    /// The widgets of the document that shows a page: what its tags expand
    /// to, as its page shows them now.
    pub(super) fn widgets(&mut self, shown: &Shown) -> Option<&Widgets> {
        if !self.widgets.contains_key(&shown.file) {
            let set = self
                .document(&shown.file)
                .map(|document| Widgets::of(&document, &self.modules));
            self.widgets.insert(shown.file.clone(), set);
        }
        self.widgets[&shown.file].as_ref()
    }
}

impl Unit {
    /// Whether a change to `path` reaches its content: its file, or its
    /// document's widgets folder — a widget added, renamed or removed
    /// changes which tags are widgets.
    pub(super) fn reached(&self, path: &Path) -> bool {
        self.file.starts_with(path)
            || self
                .shown_in
                .as_ref()
                .and_then(|shown| shown.widgets.as_ref())
                .is_some_and(|widgets| path.starts_with(widgets) || widgets.starts_with(path))
    }

    /// Where the page opens: what holds it, at the chapter's name or the
    /// slide's number. A page's address in the index.
    pub(super) fn url(&self) -> String {
        match &self.name {
            Some(name) => format!("{}#{name}", self.address),
            None => self.address.clone(),
        }
    }
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
        let Some(yaml) = crate::page::markdown::front_matter(source) else {
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
/// reach whose front matter says `search: true` comes last. With them, the
/// [footprint](Footprint) of every Markdown file in sight, and of every
/// page whose Topics a nav read.
fn scope(pass: &mut Pass, entry: &Path) -> (Vec<Unit>, HashMap<PathBuf, Footprint>) {
    let folder = pass.folder;
    let mut walk = Walk {
        pass,
        documents: BTreeSet::new(),
        units: Vec::new(),
        footprints: HashMap::new(),
    };
    walk.document(entry);
    let mut footprints = walk.footprints;
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
    for file in markdown_files(folder) {
        if seen.contains(&file) || parts.contains(&file) {
            // What of it decides what is published, the walk read.
            footprints.entry(file).or_default();
            continue;
        }
        let Some(source) = markdown(&file) else {
            continue;
        };
        let search = FrontMatter::read(&source).search;
        let footprint = footprints.entry(file.clone()).or_default();
        footprint.search = Some(search);
        if search != Some(true) {
            continue;
        }
        // The walk stays in the served folder; a file it can't name is
        // outside it, and never indexed.
        let Some(address) = folder.served().url(&file) else {
            continue;
        };
        let site = folder.site(&file).and_then(|site| pass.document(&site));
        let document = match site {
            Some(site) => Owner {
                title: site.title.clone(),
                path: site.part_url(&site.path()),
            },
            None => {
                let title = crate::markup::title(&source, &file);
                footprint.title = Some(title.clone());
                Owner {
                    title,
                    path: address.clone(),
                }
            }
        };
        units.push(Unit {
            shown_in: pass.shown(&file),
            file,
            kind: PageKind::Page,
            trail: Vec::new(),
            document,
            address,
            name: None,
            nav: 0,
        });
    }
    for (nav, unit) in units.iter_mut().enumerate() {
        unit.nav = nav as u64;
    }
    (units, footprints)
}

/// The walk from the served document through what it reaches.
struct Walk<'p, 'a> {
    pass: &'p mut Pass<'a>,
    documents: BTreeSet<PathBuf>,
    units: Vec<Unit>,
    /// What of each Markdown file it read decides what it found.
    footprints: HashMap<PathBuf, Footprint>,
}

impl Walk<'_, '_> {
    fn in_sight(&self, file: &Path) -> bool {
        !self.pass.folder.ignored(file, false)
    }

    fn document(&mut self, file: &Path) {
        if !self.documents.insert(file.to_path_buf()) || !self.in_sight(file) {
            return;
        }
        let Some(document) = self.pass.document(file) else {
            return;
        };
        let owner = Owner {
            title: document.title.clone(),
            path: document.part_url(&document.path()),
        };
        match &document.kind {
            Kind::Site { root } => {
                let (tree, topics) = crate::nav::tree_and_topics(&document);
                for (page, topics) in topics {
                    self.footprints.entry(page).or_default().topics = Some(topics);
                }
                let trails = trails(&document, &tree);
                // A page outside the served folder has no address, and is
                // never indexed.
                let page = |walk: &mut Self, file: &Path| {
                    if walk.in_sight(file)
                        && let Some(address) = document.url(file)
                    {
                        walk.units.push(Unit {
                            shown_in: walk.pass.shown(file),
                            file: file.to_path_buf(),
                            kind: PageKind::Page,
                            trail: trails.get(file).cloned().unwrap_or_default(),
                            document: owner.clone(),
                            address,
                            name: None,
                            nav: 0,
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
                listed(&tree, &mut files);
                for file in files {
                    if file.extension().is_some_and(|ext| ext == "md") {
                        page(self, &file);
                    } else {
                        self.document(&file);
                    }
                }
                for file in crate::page::navbar::reached(&document).0 {
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
                        let trail = markdown(chapter)
                            .map(|source| vec![crate::markup::title(&source, chapter)])
                            .unwrap_or_default();
                        self.footprints.entry(chapter.clone()).or_default().trail =
                            Some(trail.clone());
                        trail
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
                        nav: 0,
                        shown_in: Some(Shown::of(&document)),
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
                        nav: 0,
                        shown_in: Some(Shown::of(&document)),
                    });
                }
            }
            Kind::Design => {}
        }
    }
}

/// Every in-sight Markdown file of `folder`. The walk honors its
/// `.gitignore` files, as the document index's does, so it never descends
/// into a build folder.
pub(super) fn markdown_files(folder: &Folder) -> Vec<PathBuf> {
    let walk = WalkBuilder::new(folder.served().path())
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
        .filter(|path| !folder.ignored(path, false))
        .collect();
    files.sort();
    files
}

/// Each page in `tree`, a site's nav, with the labels from the top of the
/// tree down to it.
pub(super) fn trails(
    document: &Document,
    tree: &[crate::nav::Group],
) -> BTreeMap<PathBuf, Vec<String>> {
    pub(super) fn walk(
        groups: &[crate::nav::Group],
        above: &[String],
        out: &mut BTreeMap<PathBuf, Vec<String>>,
    ) {
        for entry in groups.iter().flat_map(|group| &group.entries) {
            let mut trail = above.to_vec();
            trail.push(entry.label.clone());
            walk(&entry.children, &trail, out);
            out.insert(entry.file.clone(), trail);
        }
    }
    let mut out = BTreeMap::new();
    if let Kind::Site { root } = &document.kind {
        out.insert(document.base.join(root), Vec::new());
    }
    walk(tree, &[], &mut out);
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

/// A page's content as its page shows it — `rendered`, its widgets' tags
/// expanded — split at its headings, each heading with the id the page
/// gives it ([`Rendered::heading_ids`]), and its text sorted by kind: prose
/// — with a diagram's labels and a widget's text — and code. A heading
/// inside a widget is a section; one inside an unknown tag, which the page
/// shows as an error, is not.
pub(super) fn read(rendered: &Rendered) -> Read {
    let source = rendered.expanded();
    let arena = Arena::new();
    let root = parse_document(&arena, source, &crate::page::markdown::options());
    let lede = root
        .children()
        .find(|node| matches!(node.data().value, NodeValue::Paragraph))
        .map(|node| squeeze(&node.collect_text()))
        .unwrap_or_default();
    let mut ids = rendered.heading_ids().into_iter();
    let mut sections = vec![Section::default()];
    let mut stack: Vec<&AstNode> = root.reverse_children().collect();
    while let Some(node) = stack.pop() {
        if let NodeValue::Heading(heading) = node.data().value {
            let text = node.collect_text();
            sections.push(Section {
                heading: text.trim().to_string(),
                id: ids.next(),
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
            // A slide's `<Notes>` were taken out with the widgets' tags
            // expanded: a deck adds them to its slide's text itself.
            NodeValue::HtmlBlock(block) => {
                current.prose.push(' ');
                current.prose.push_str(&widget_text(&block.literal));
                current.prose.push(' ');
            }
            NodeValue::SoftBreak | NodeValue::LineBreak => current.prose.push(' '),
            NodeValue::HtmlInline(_) | NodeValue::FrontMatter(_) => {}
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
    // A widget's placeholder: its name, and its content's source, which is
    // read as the page's own Markdown.
    "data-widget",
    "data-widget-source",
    "data-widget-source-html",
];

/// What an unknown widget tag becomes, as a block or inline: its error, in
/// its place.
const WIDGET_ERRORS: [(&str, &str); 2] = [
    ("<div class=\"widget-error\">", "</div>"),
    ("<span class=\"widget-error\">", "</span>"),
];

/// `html` without the errors unknown widget tags became: no text of the
/// page's. Their message is escaped, so it holds no tag.
fn without_errors(html: &str) -> String {
    let mut html = html.to_string();
    for (open, close) in WIDGET_ERRORS {
        while let Some(at) = html.find(open) {
            let end = html[at..]
                .find(close)
                .map_or(html.len(), |end| at + end + close.len());
            html.replace_range(at..end, " ");
        }
    }
    html
}

/// The text a widget's props hold, from the object literal its placeholder
/// carries — `{"title": "…", "values": ([…]), "open": true}`: the value of
/// each string prop whose name is text, never an expression's.
fn props_text(props: &str) -> String {
    let mut text = Vec::new();
    let mut rest = props.trim().strip_prefix('{').unwrap_or_default();
    loop {
        rest = rest.trim_start_matches([' ', ',']);
        let mut strings = serde_json::Deserializer::from_str(rest).into_iter::<String>();
        let Some(Ok(key)) = strings.next() else {
            break;
        };
        rest = rest[strings.byte_offset()..].trim_start();
        let Some(value) = rest.strip_prefix(':') else {
            break;
        };
        rest = value.trim_start();
        if rest.starts_with('"') {
            let mut strings = serde_json::Deserializer::from_str(rest).into_iter::<String>();
            let Some(Ok(value)) = strings.next() else {
                break;
            };
            rest = &rest[strings.byte_offset()..];
            if !NOT_TEXT.contains(&key.as_str()) && !value.contains("://") {
                text.push(value);
            }
        } else if rest.starts_with('(') {
            rest = &rest[expression_end(rest)..];
        } else {
            // A bare attribute: `true`.
            rest = rest.trim_start_matches(|c: char| c.is_ascii_alphanumeric());
        }
    }
    text.join(" ")
}

/// The length of the parenthesized expression `text` starts with, its
/// strings skipped; all of it when it never closes.
fn expression_end(text: &str) -> usize {
    let mut depth = 0;
    let mut string: Option<char> = None;
    let mut escaped = false;
    for (at, c) in text.char_indices() {
        if let Some(quote) = string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == quote {
                string = None;
            }
            continue;
        }
        match c {
            '"' | '\'' | '`' => string = Some(c),
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return at + 1;
                }
            }
            _ => {}
        }
    }
    text.len()
}

/// The text of an HTML block: what is between its tags, and its tags' text
/// attributes — a widget's text props among them — never its markup, an
/// expression `{…}`, an address, or an unknown tag's error.
pub(super) fn widget_text(html: &str) -> String {
    let html = without_errors(html);
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
                if name == "data-props" {
                    text.push(' ');
                    text.push_str(&props_text(&crate::html::unescape(&value)));
                    text.push(' ');
                } else if !NOT_TEXT.contains(&name.as_str()) && !value.contains("://") {
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
