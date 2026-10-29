//! The content phase: one file's Markdown, its widgets' tags expanded, as
//! the HTML its page shows — with every link that content holds, by file and
//! line. The frame is put round it afterwards, so no link of the frame's —
//! the navbar's, the sidebar's — is among them. What the preview and
//! `devpage check` judge a page by is this, the content its page shows.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::document::{Document, Kind};
use crate::folder::Folder;
use crate::html;
use crate::widgets::{Modules, Widgets};

use super::frame::chapter_names;
use super::markdown::markdown;

/// One file's content as its page shows it.
pub struct Rendered {
    file: PathBuf,
    source: String,
    expanded: String,
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

    /// The Markdown its HTML is made from: the source with its widgets'
    /// tags expanded, `<Notes>` taken out and an unknown tag replaced by
    /// its error — what search reads a page by.
    pub fn expanded(&self) -> &str {
        &self.expanded
    }

    /// The content's HTML, links as written.
    pub fn html(&self) -> &str {
        &self.html
    }

    /// Every id the content's HTML holds — its headings', a footnote's, a
    /// raw element's — a link's `#fragment` can land on.
    pub fn ids(&self) -> BTreeSet<String> {
        html::values(&self.html, "id").into_iter().collect()
    }

    /// The id of each Markdown heading the content shows, in order — a
    /// heading inside a widget among them, one inside an unknown tag not,
    /// since its page doesn't show it.
    pub fn heading_ids(&self) -> Vec<String> {
        super::markdown::heading_ids(&self.html)
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
    let _render = tracing::info_span!("render").entered();
    let mut tags = Vec::new();
    let expanded = crate::widgets::expand(&source, widgets.names(), &mut tags);
    let html = markdown(&expanded);
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
        expanded,
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

/// The document a Markdown file is shown in at its own address: its nearest
/// site; in no site, the file alone, as a doc of one chapter. `None` when
/// its site can't be read: its page then shows why, and none of the file.
pub fn shown_in(file: &Path, folder: &Folder) -> Option<Document> {
    Document::load(&shown_by(file, folder), folder.served()).ok()
}

/// The file of the document a Markdown file is [shown in](shown_in),
/// unloaded: its nearest site's document file, or in no site the file
/// itself.
pub fn shown_by(file: &Path, folder: &Folder) -> PathBuf {
    folder.site(file).unwrap_or_else(|| file.to_path_buf())
}

/// Every id a link's `#fragment` into `file` can land on, as its page
/// renders them: the ids of the content phase, the tags of its document's
/// widgets expanded, read through `modules` — without building the frame.
/// A Markdown file's are its content's, as the document it is
/// [shown in](shown_in) renders it; a doc's are its chapters', each
/// prefixed with its chapter's name when there is more than one, and the
/// chapters' own names. Anything else has none.
pub fn ids(file: &Path, folder: &Folder, modules: &Modules) -> BTreeSet<String> {
    if file.extension().is_some_and(|ext| ext == "md") {
        return shown_in(file, folder)
            .map(|document| read(&Widgets::of(&document, modules), file).ids())
            .unwrap_or_default();
    }
    let served = folder.served();
    let Ok(document) = Document::load(file, served) else {
        return BTreeSet::new();
    };
    let Kind::Doc { chapters, .. } = &document.kind else {
        return BTreeSet::new();
    };
    let widgets = Widgets::of(&document, modules);
    let own_ids = |chapter: &Path| read(&widgets, chapter).ids();
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
