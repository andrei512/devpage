//! The content phase: one file's Markdown, its widgets' tags expanded, as
//! the HTML its page shows — with every link that content holds, by file and
//! line. The frame is put round it afterwards, so no link of the frame's —
//! the navbar's, the sidebar's — is among them. What the preview and
//! `devpage check` judge a page by is this, the content its page shows.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::document::{Document, Kind};
use crate::folder::ServedFolder;
use crate::html;
use crate::widgets::Widgets;

use super::frame::chapter_names;
use super::markdown::markdown;

/// One file's content as its page shows it.
pub struct Rendered {
    file: PathBuf,
    source: String,
    html: String,
    links: Vec<Link>,
    embeds: Vec<String>,
    tags: Vec<String>,
}

/// A link the content holds: what it names as written — an `href`, a
/// `src`, or a design the Markdown embeds — the file it is written in, and
/// the line, when its text is found in the file.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub file: PathBuf,
    pub line: Option<usize>,
    pub target: String,
}

impl Rendered {
    /// The file the content is read from.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// The Markdown as written.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The content's HTML, links as written.
    pub fn html(&self) -> &str {
        &self.html
    }

    /// Every link the content holds, in the order its HTML holds them.
    pub fn links(&self) -> &[Link] {
        &self.links
    }

    /// What the content draws from other files, as written: every `src` —
    /// an image, a design's canvas, a frame — and every design the Markdown
    /// embeds. A link only to be followed is not among them.
    pub fn embeds(&self) -> &[String] {
        &self.embeds
    }

    /// What expanding the widgets' tags found wrong — a tag that is no
    /// widget, a misplaced one — each as `line: message`.
    pub(crate) fn tag_problems(&self) -> &[String] {
        &self.tags
    }
}

/// `file`'s content from `source`, the tags of `widgets` expanded first.
pub fn content(widgets: &Widgets, file: &Path, source: String) -> Rendered {
    let mut tags = Vec::new();
    let html = markdown(&crate::widgets::expand(&source, widgets.names(), &mut tags));
    let mut targets = html::values(&html, "href");
    let mut embeds = html::values(&html, "src");
    targets.extend(embeds.iter().cloned());
    // A design embedded with `![…](x.design)`, read from the Markdown itself,
    // whatever HTML the page renders it as.
    for design in embedded_designs(&source) {
        if !embeds.contains(&design) {
            embeds.push(design.clone());
        }
        if !targets.contains(&design) {
            targets.push(design);
        }
    }
    let links = targets
        .into_iter()
        .map(|target| Link {
            file: file.to_path_buf(),
            line: source
                .find(&target)
                .map(|at| source[..at].lines().count().max(1)),
            target,
        })
        .collect();
    Rendered {
        file: file.to_path_buf(),
        source,
        html,
        links,
        embeds,
        tags,
    }
}

/// `file`'s content as read from the disk now; a file that can't be read
/// has none.
pub fn read(widgets: &Widgets, file: &Path) -> Rendered {
    content(
        widgets,
        file,
        std::fs::read_to_string(file).unwrap_or_default(),
    )
}

/// Every id a link's `#fragment` into `file` can land on, as its page
/// renders them — without building the page. A Markdown file's are its
/// content's ids; a doc's are its chapters', each prefixed with its
/// chapter's name when there is more than one, and the chapters' own names.
/// Anything else has none.
pub fn ids(file: &Path, served: &ServedFolder) -> BTreeSet<String> {
    if file.extension().is_some_and(|ext| ext == "md") {
        return own_ids(file);
    }
    let Ok(document) = Document::load(file, served) else {
        return BTreeSet::new();
    };
    let Kind::Doc { chapters, .. } = &document.kind else {
        return BTreeSet::new();
    };
    if chapters.len() == 1 {
        return own_ids(&chapters[0]);
    }
    let names = chapter_names(chapters, served);
    let mut ids = BTreeSet::new();
    for (chapter, (_, name)) in chapters.iter().zip(&names) {
        ids.extend(
            own_ids(chapter)
                .into_iter()
                .map(|id| format!("{name}--{id}")),
        );
        ids.insert(name.clone());
    }
    ids
}

/// The ids a Markdown file's content renders.
fn own_ids(file: &Path) -> BTreeSet<String> {
    let html = markdown(&std::fs::read_to_string(file).unwrap_or_default());
    html::values(&html, "id").into_iter().collect()
}

/// The `.design` files a page's Markdown embeds or links to, read from its
/// syntax tree — never from code, where `![…](x.design)` is only an example.
fn embedded_designs(source: &str) -> Vec<String> {
    use comrak::nodes::NodeValue;
    let arena = comrak::Arena::new();
    let root = comrak::parse_document(&arena, source, &comrak::Options::default());
    root.descendants()
        .filter_map(|node| match &node.data.borrow().value {
            NodeValue::Image(link) | NodeValue::Link(link) => Some(link.url.clone()),
            _ => None,
        })
        .filter(|url| {
            url.split('#')
                .next()
                .is_some_and(|path| path.ends_with(".design"))
        })
        .collect()
}
