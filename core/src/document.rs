//! A document: what a document file says to show — a site, a doc or slides —
//! or a lone Markdown file, which is a doc of one chapter.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::folder::ServedFolder;
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;

#[cfg(test)]
thread_local! {
    /// Every document file loaded on this thread, for the tests: how often a
    /// pass loads each.
    pub(crate) static LOADED: std::cell::RefCell<Vec<PathBuf>> = Default::default();
}

/// The YAML as written, tagged by `kind`. Paths are relative to its folder.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
enum Config {
    /// A site: pages reached from its `root` page, with a navbar and the Topics
    /// sidebar if it asks for them.
    Site {
        title: String,
        root: String,
        watch: Option<Vec<String>>,
        widgets: Option<String>,
        navbar: Option<serde_yaml::Mapping>,
        sidebar: Option<bool>,
        #[serde(rename = "theme-selector")]
        theme_selector: Option<bool>,
        logo: Option<String>,
        #[serde(default)]
        extensions: std::collections::BTreeMap<String, crate::extensions::Config>,
    },
    /// A doc: its `chapters` in order, and its settings on paper.
    Doc {
        title: String,
        chapters: Vec<String>,
        #[serde(default)]
        pdf: Pdf,
        widgets: Option<String>,
        #[serde(rename = "theme-selector")]
        theme_selector: Option<bool>,
        logo: Option<String>,
        #[serde(default)]
        extensions: std::collections::BTreeMap<String, crate::extensions::Config>,
    },
    /// A deck: its `slides` in order, and what its narrator is told about the
    /// talk.
    Slides {
        title: String,
        slides: Vec<String>,
        audience: Option<String>,
        goal: Option<String>,
        tone: Option<String>,
        pronounce: Option<Vec<String>>,
        widgets: Option<String>,
        #[serde(rename = "theme-selector")]
        theme_selector: Option<bool>,
        logo: Option<String>,
        #[serde(default)]
        extensions: std::collections::BTreeMap<String, crate::extensions::Config>,
    },
}

/// One link of a site's navbar, as written: a label, an icon or both, and its
/// target — a path relative to the document file, or an address on the web.
#[derive(Debug, Clone, PartialEq)]
pub struct NavLink {
    /// Its text; `None` when it shows only its icon.
    pub label: Option<String>,
    /// The name of its icon, from the key's `(icon)`; `None` when it has none.
    pub icon: Option<String>,
    /// Where it leads, as written: a path relative to the document file, or an
    /// address on the web.
    pub target: String,
}

/// An entry of a site's navbar: a link, or a dropdown of links.
#[derive(Debug, Clone, PartialEq)]
pub enum NavItem {
    /// A link on the navbar itself.
    Link(NavLink),
    /// A dropdown, with its own label or icon, of `items`.
    Menu {
        label: Option<String>,
        icon: Option<String>,
        items: Vec<NavLink>,
    },
}

/// Splits a navbar key into its label and its `(icon)`: `Docs (book-open)`,
/// `Docs`, or `(github)` alone.
fn label_and_icon(key: &str) -> Result<(Option<String>, Option<String>)> {
    let key = key.trim();
    let (label, icon) = match key.strip_suffix(')').and_then(|rest| rest.rsplit_once('(')) {
        Some((label, icon))
            if !icon.is_empty()
                && icon
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') =>
        {
            (label.trim(), Some(icon.to_string()))
        }
        _ => (key, None),
    };
    let label = (!label.is_empty()).then(|| label.to_string());
    if label.is_none() && icon.is_none() {
        bail!("navbar has an entry with neither a label nor an icon");
    }
    Ok((label, icon))
}

/// Reads a `navbar` map, in order: `label: target` is a link, `label:` with a
/// nested map is a dropdown of links, one level deep.
fn navbar(map: &serde_yaml::Mapping) -> Result<Vec<NavItem>> {
    let key_text = |key: &serde_yaml::Value| -> Result<String> {
        match key {
            serde_yaml::Value::String(text) => Ok(text.clone()),
            other => bail!("navbar: {other:?} is not a label"),
        }
    };
    let mut items = Vec::new();
    for (key, value) in map {
        let key = key_text(key)?;
        let (label, icon) = label_and_icon(&key)?;
        match value {
            serde_yaml::Value::String(target) => items.push(NavItem::Link(NavLink {
                label,
                icon,
                target: target.clone(),
            })),
            serde_yaml::Value::Mapping(links) => {
                let mut menu = Vec::new();
                for (inner_key, value) in links {
                    let inner = key_text(inner_key)?;
                    let serde_yaml::Value::String(target) = value else {
                        bail!(
                            "navbar: {inner} under {key} must be a link — dropdowns are one level deep"
                        );
                    };
                    let (label, icon) = label_and_icon(&inner)?;
                    menu.push(NavLink {
                        label,
                        icon,
                        target: target.clone(),
                    });
                }
                if menu.is_empty() {
                    bail!("navbar: {key} lists no links");
                }
                items.push(NavItem::Menu {
                    label,
                    icon,
                    items: menu,
                });
            }
            _ => bail!("navbar: {key} must be a link or a map of links"),
        }
    }
    Ok(items)
}

/// A doc's settings on paper.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Pdf {
    /// The paper size each page is printed on.
    #[serde(default)]
    pub paper: Paper,
    /// The text at the top of every page, where `{page}` and `{pages}` become
    /// page numbers; `None` leaves it blank.
    pub header: Option<String>,
    /// The text at the bottom of every page, numbered as `header` is: `{page} /
    /// {pages}` unless set.
    #[serde(default = "default_footer")]
    pub footer: String,
}

impl Default for Pdf {
    fn default() -> Self {
        Self {
            paper: Paper::default(),
            header: None,
            footer: default_footer(),
        }
    }
}

/// The footer a doc gets when it sets none: `{page} / {pages}`.
fn default_footer() -> String {
    "{page} / {pages}".into()
}

/// The paper size a doc is printed on.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq)]
pub enum Paper {
    /// ISO A4: 210 × 297 mm, the default.
    #[default]
    A4,
    /// US Letter: 8.5 × 11 in.
    Letter,
}

/// What a document is, with its parts resolved to files.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// A site, whose `/` leads to `root`: its page as written, relative to the
    /// document's folder.
    Site { root: String },
    /// A doc: its chapter files in order, and its settings on paper.
    Doc { chapters: Vec<PathBuf>, pdf: Pdf },
    /// A deck: its slides in order.
    Slides {
        slides: Vec<Slide>,
        /// Who the talk is for and how it should sound, for its narrator.
        narration: Narration,
    },
    /// A `.design` file, which is the whole document.
    Design,
}

/// What a deck tells its narrator about the talk as a whole, as written in
/// its document file: who it is for, what it should achieve, how it should
/// sound, and how to say words a voice gets wrong.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Narration {
    /// Who the talk is for.
    pub audience: Option<String>,
    /// What the talk should achieve.
    pub goal: Option<String>,
    /// How the talk should sound; `None` leaves the narrator's default tone.
    pub tone: Option<String>,
    /// How to say the words a voice gets wrong, one instruction each.
    pub pronounce: Vec<String>,
}

/// One slide of a deck: its Markdown file, its notes — the text of its first
/// `<Notes>`, if it has one — and that `<Notes>`' `delivery`.
#[derive(Debug, Clone, PartialEq)]
pub struct Slide {
    /// The slide's Markdown file.
    pub file: PathBuf,
    /// The text of its first `<Notes>`; `None` when it has none.
    pub notes: Option<String>,
    /// That `<Notes>`' `delivery`: how the narrator should say it.
    pub delivery: Option<String>,
}

impl Slide {
    /// Reads a slide's notes from its file.
    fn read(file: PathBuf) -> Self {
        let source = std::fs::read_to_string(&file).unwrap_or_default();
        let first = crate::markup::notes(&source).into_iter().next();
        let (notes, delivery) = match first {
            Some(note) => (Some(note.text), note.delivery),
            None => (None, None),
        };
        Self {
            file,
            notes,
            delivery,
        }
    }
}

/// Which kind a document file names, read without loading the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindName {
    /// A site.
    Site,
    /// A doc.
    Doc,
    /// A deck of slides.
    Slides,
}

/// A loaded document: its title, its folder, what it is, and where it is served.
#[derive(Debug)]
pub struct Document {
    /// The extensions the document file configures, bound to it.
    pub extensions: Vec<crate::extensions::Binding>,
    /// The title it shows: as written, or a Markdown file's first heading, or a
    /// design's file name.
    pub title: String,
    /// The folder of the document file, which its paths are relative to.
    pub base: PathBuf,
    /// What it is — a site, a doc, slides or a design — with its parts
    /// resolved.
    pub kind: Kind,
    /// The document file's name inside `base`.
    pub file_name: String,
    /// The served folder: where devpage was started, or `--root`. Addresses
    /// are paths in it, and nothing outside it is served.
    pub served: ServedFolder,
    /// A site's `watch` list: when set, only matching files refresh pages.
    watch: Option<GlobSet>,
    /// Where each `watch` pattern can match: its literal prefix — the
    /// components before the first with a glob character — joined to `base`.
    watch_roots: Vec<PathBuf>,
    /// The folder of the document's project widgets, if it has one.
    pub widgets: Option<PathBuf>,
    /// A site's navbar, in order; empty when it has none.
    pub navbar: Vec<NavItem>,
    /// Whether a site shows its Topics sidebar.
    pub sidebar: bool,
    /// Whether the page shows the light/dark selector.
    pub theme_selector: bool,
    /// The `logo` field as written: an image relative to the document file,
    /// shown before the title and used as the browser tab's icon.
    pub logo: Option<String>,
}

impl Document {
    /// Loads a document file, `devpage.yml` in a folder, a Markdown file or a
    /// design, which must lie inside the served folder `served`.
    pub fn load(path: &Path, served: &ServedFolder) -> Result<Self> {
        let file = if path.is_dir() {
            let file = path.join("devpage.yml");
            if !file.exists() {
                bail!("{}", no_document_file(path));
            }
            file
        } else {
            path.to_path_buf()
        };
        #[cfg(test)]
        LOADED.with_borrow_mut(|loaded| loaded.push(file.clone()));
        let text = std::fs::read_to_string(&file)
            .with_context(|| format!("reading {}", file.display()))?;
        std::fs::metadata(served.path())
            .with_context(|| format!("reading {}", served.path().display()))?;
        // A bare file name has an empty parent, which means the current folder.
        let parent = file.parent().filter(|p| !p.as_os_str().is_empty());
        let parent = parent.unwrap_or(Path::new("."));
        let Some(base) = served.folder(parent) else {
            parent.canonicalize()?;
            bail!(
                "{} is outside the served folder {}",
                path.display(),
                served.path().display()
            );
        };
        let file_name = file
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if file.extension().is_some_and(|ext| ext == "design") {
            return Ok(Self {
                extensions: Vec::new(),
                served: served.clone(),
                title: file
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                kind: Kind::Design,
                base,
                file_name,
                watch: None,
                watch_roots: Vec::new(),
                widgets: None,
                navbar: Vec::new(),
                sidebar: true,
                theme_selector: true,
                logo: None,
            });
        }
        if file.extension().is_some_and(|ext| ext == "md") {
            return Ok(Self {
                extensions: Vec::new(),
                served: served.clone(),
                title: crate::markup::title(&text, &file),
                kind: Kind::Doc {
                    chapters: vec![base.join(&file_name)],
                    pdf: Pdf::default(),
                },
                base,
                file_name,
                watch: None,
                watch_roots: Vec::new(),
                widgets: None,
                navbar: Vec::new(),
                sidebar: true,
                theme_selector: true,
                logo: None,
            });
        }
        // Named as the served folder names it, with `/` on every platform.
        let named = served
            .name(&base.join(&file_name))
            .unwrap_or_else(|| file.display().to_string());
        Self::parse(&text, base, file_name, served.clone()).with_context(|| format!("in {named}"))
    }

    /// Reads a document file's YAML `text`, whose paths are relative to `base`,
    /// checking each part is an existing file in `served`.
    fn parse(text: &str, base: PathBuf, file_name: String, served: ServedFolder) -> Result<Self> {
        // A part must be an existing file inside the served folder.
        let existing = |field: &str, path: &str| -> Result<PathBuf> {
            match served.file(&base.join(path)) {
                Some(file) => Ok(file),
                None => bail!("{field} {path} does not exist"),
            }
        };
        let mut watch = None;
        let mut watch_roots = Vec::new();
        let widget_folder;
        let mut nav_items = Vec::new();
        let mut show_sidebar = true;
        let show_selector;
        let logo_path;
        let extension_configs;
        let (title, kind) = match serde_yaml::from_str(text)? {
            Config::Site {
                title,
                root,
                watch: patterns,
                widgets,
                navbar: nav,
                sidebar,
                theme_selector,
                logo,
                extensions,
            } => {
                extension_configs = extensions;
                logo_path = logo;
                widget_folder = widgets;
                show_selector = theme_selector.unwrap_or(true);
                show_sidebar = sidebar.unwrap_or(true);
                if let Some(nav) = nav {
                    nav_items = navbar(&nav)?;
                }
                existing("root", &root)?;
                if let Some(patterns) = patterns {
                    let mut set = GlobSetBuilder::new();
                    for pattern in &patterns {
                        set.add(
                            Glob::new(pattern)
                                .with_context(|| format!("watch pattern {pattern}"))?,
                        );
                    }
                    watch = Some(set.build()?);
                    watch_roots = patterns
                        .iter()
                        .map(|pattern| base.join(literal_prefix(pattern)))
                        .collect();
                }
                (title, Kind::Site { root })
            }
            Config::Doc {
                title,
                chapters,
                pdf,
                widgets,
                theme_selector,
                logo,
                extensions,
            } => {
                extension_configs = extensions;
                logo_path = logo;
                widget_folder = widgets;
                show_selector = theme_selector.unwrap_or(true);
                if chapters.is_empty() {
                    bail!("chapters lists no files");
                }
                let chapters = chapters
                    .iter()
                    .map(|chapter| existing("chapter", chapter))
                    .collect::<Result<_>>()?;
                (title, Kind::Doc { chapters, pdf })
            }
            Config::Slides {
                title,
                slides,
                audience,
                goal,
                tone,
                pronounce,
                widgets,
                theme_selector,
                logo,
                extensions,
            } => {
                extension_configs = extensions;
                logo_path = logo;
                widget_folder = widgets;
                show_selector = theme_selector.unwrap_or(true);
                if slides.is_empty() {
                    bail!("slides lists no files");
                }
                let slides = slides
                    .iter()
                    .map(|slide| existing("slide", slide).map(Slide::read))
                    .collect::<Result<_>>()?;
                let narration = Narration {
                    audience,
                    goal,
                    tone,
                    pronounce: pronounce.unwrap_or_default(),
                };
                (title, Kind::Slides { slides, narration })
            }
        };
        let widgets = match widget_folder {
            Some(folder) => match served.folder(&base.join(&folder)) {
                Some(dir) => Some(dir),
                None => bail!("widgets {folder} is not a folder"),
            },
            None => None,
        };
        let mut extension_configs = extension_configs;
        // `code` with no package is the system extension: the code reference,
        // which only a site has pages for.
        let code = match extension_configs.get("code") {
            Some(config) if config.package.is_none() => {
                if !matches!(kind, Kind::Site { .. }) {
                    bail!("extensions: code: only a site has a code reference");
                }
                extension_configs.remove("code").map(|config| config.config)
            }
            _ => None,
        };
        crate::code::configure(&base.join(&file_name), &base, &served, code.as_ref())?;
        crate::extensions::configure(&base.join(&file_name), &base, &served, extension_configs)?;
        Ok(Self {
            extensions: crate::extensions::for_document(&base.join(&file_name)),
            title,
            base,
            kind,
            file_name,
            served,
            watch,
            watch_roots,
            widgets,
            navbar: nav_items,
            sidebar: show_sidebar,
            theme_selector: show_selector,
            logo: logo_path,
        })
    }

    /// Where `/` leads: a site's root page, or the document file itself.
    pub fn entry(&self) -> String {
        match &self.kind {
            Kind::Site { root } => self.part_url(&self.base.join(root)),
            _ => self.part_url(&self.path()),
        }
    }

    /// The address of one of the document's parts — its file, a site's
    /// root, a chapter or a slide.
    ///
    /// Panics on any other file outside the served folder: [`Document::load`]
    /// refuses a document whose part lies outside it, so a part always has
    /// an address. Any other file's is [`Document::url`].
    pub fn part_url(&self, part: &Path) -> String {
        self.part(part, self.url(part))
    }

    /// The name of one of the document's parts, as its problems show it;
    /// panics as [`Document::part_url`] does.
    pub fn part_name(&self, part: &Path) -> String {
        self.part(part, self.name(part))
    }

    /// Unwraps `named`, the address or name of `part`: panics when it is
    /// `None`, as [`Document::part_url`] says.
    fn part(&self, part: &Path, named: Option<String>) -> String {
        named.unwrap_or_else(|| {
            panic!(
                "{} is no part of {}: it lies outside the served folder {}",
                part.display(),
                self.path().display(),
                self.served.path().display()
            )
        })
    }

    /// The document file itself.
    pub fn path(&self) -> PathBuf {
        self.base.join(&self.file_name)
    }

    /// The logo's address, when the document has one and the file exists in
    /// the served folder. A logo that doesn't is a problem, not an error:
    /// the page shows without it.
    pub fn logo_url(&self) -> Option<String> {
        let written = self.logo.as_ref()?;
        let file = self.served.file(&self.base.join(written))?;
        self.url(&file)
    }

    /// A file's address: its path from the top of the served folder;
    /// `None` for a file outside it, which has none.
    pub fn url(&self, file: &Path) -> Option<String> {
        self.served.url(file)
    }

    /// A file's name from the top of the served folder, for labels; `None`
    /// for a file outside it, which is never named.
    pub fn name(&self, file: &Path) -> Option<String> {
        self.served.name(file)
    }

    /// The file a URL path names, if it lies inside the served folder.
    pub fn file(&self, url_path: &str) -> Option<PathBuf> {
        self.served.at(url_path)
    }

    /// Whether a change to `file` refreshes open pages: always, unless this
    /// is a site whose `watch` list matches none of it, relative to its folder.
    pub fn watches(&self, file: &Path) -> bool {
        let Some(watch) = &self.watch else {
            return true;
        };
        file.strip_prefix(&self.base)
            .is_ok_and(|relative| watch.is_match(relative))
    }

    /// Whether this is a site whose `watch` list names `file`, relative to
    /// its folder.
    pub fn names(&self, file: &Path) -> bool {
        self.watch.is_some() && self.watches(file)
    }

    /// The folders a site's `watch` patterns can name files in: each
    /// pattern's literal prefix in the site's folder. Empty without a list.
    pub fn watch_roots(&self) -> &[PathBuf] {
        &self.watch_roots
    }
}

/// A glob pattern's components before the first one with a glob character:
/// `build/*.svg` gives `build`, `**/*.md` gives nothing.
fn literal_prefix(pattern: &str) -> PathBuf {
    Path::new(pattern)
        .components()
        .take_while(|part| {
            !part
                .as_os_str()
                .to_string_lossy()
                .contains(['*', '?', '[', '{'])
        })
        .collect()
}

/// The kind a YAML file names in its top-level `kind`, when it is one of
/// devpage's three; any other file is not a document file.
pub fn kind_of(file: &Path) -> Option<KindName> {
    if !file
        .extension()
        .is_some_and(|ext| ext == "yml" || ext == "yaml")
    {
        return None;
    }
    let text = std::fs::read_to_string(file).ok()?;
    let value: serde_yaml::Value = serde_yaml::from_str(&text).ok()?;
    match value.get("kind")?.as_str()? {
        "site" => Some(KindName::Site),
        "doc" => Some(KindName::Doc),
        "slides" => Some(KindName::Slides),
        _ => None,
    }
}

/// What a published document of `kind` downloads as — a doc or slides
/// printed as a PDF, a design painted as its PNG — as its extension and
/// media type; `None` for a kind that has no download, a site. The one rule
/// for it, the server's and `devpage download`'s.
pub fn downloads_as(kind: &str) -> Option<(&'static str, &'static str)> {
    match kind {
        "doc" | "slides" => Some(("pdf", "application/pdf")),
        "design" => Some(("png", "image/png")),
        _ => None,
    }
}

/// The file a document is saved as when nothing names one — what `devpage
/// pdf` and `devpage video` write and what a version downloads as: its
/// `title`, with the characters a file name can't hold replaced by `-`, and
/// `extension`.
pub fn download_name(title: &str, extension: &str) -> PathBuf {
    let name: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    let name = name.trim().trim_start_matches('.');
    let name = if name.is_empty() { "document" } else { name };
    PathBuf::from(format!("{name}.{extension}"))
}

/// What to give devpage instead, for a folder holding no `devpage.yml`: its
/// `README.md` when it has one, else any Markdown file.
fn no_document_file(folder: &Path) -> String {
    let here = folder == Path::new(".");
    let named = |file: &str| {
        if here {
            file.to_string()
        } else {
            folder.join(file).display().to_string()
        }
    };
    let file = if folder.join("README.md").is_file() {
        named("README.md")
    } else {
        named("<file>.md")
    };
    let folder = if here {
        "the current folder".to_string()
    } else {
        folder.display().to_string()
    };
    format!(
        "{folder} has no devpage.yml\n\n  \
         Name a Markdown file instead, e.g. {file}\n  \
         Or make the folder a site: https://devpage.ai/docs/your-first-site-doc-and-deck.md"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_without_a_document_file_says_what_to_run() {
        let dir = folder("no-yml");
        let error = Document::load(&dir, &ServedFolder::canonical(dir.clone()))
            .unwrap_err()
            .to_string();
        assert!(error.contains("has no devpage.yml"), "{error}");
        assert!(
            error.contains(&format!("e.g. {}", dir.join("README.md").display())),
            "{error}"
        );
        let bare = dir.join("guides");
        let error = Document::load(&bare, &ServedFolder::canonical(dir.clone()))
            .unwrap_err()
            .to_string();
        assert!(error.contains("<file>.md"), "{error}");
    }

    #[test]
    fn docs_and_slides_download_as_a_pdf_a_design_as_a_png_and_a_site_not() {
        assert_eq!(downloads_as("doc"), Some(("pdf", "application/pdf")));
        assert_eq!(downloads_as("slides"), Some(("pdf", "application/pdf")));
        assert_eq!(downloads_as("design"), Some(("png", "image/png")));
        assert_eq!(downloads_as("site"), None);
    }

    #[test]
    fn the_download_name_is_the_title_made_safe() {
        assert_eq!(
            download_name("Offline sync", "pdf"),
            Path::new("Offline sync.pdf")
        );
        assert_eq!(
            download_name("A/B: why?", "pdf"),
            Path::new("A-B- why-.pdf")
        );
        assert_eq!(download_name("...", "pdf"), Path::new("document.pdf"));
        assert_eq!(
            download_name("A/B: v1.2", "mp4"),
            Path::new("A-B- v1.2.mp4")
        );
        assert_eq!(download_name("Screen", "png"), Path::new("Screen.png"));
    }

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("devpage-doc-{name}-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("guides")).unwrap();
        std::fs::write(dir.join("README.md"), "# Home").unwrap();
        std::fs::write(dir.join("guides/start.md"), "# Start").unwrap();
        dir.canonicalize().unwrap()
    }

    fn parse(text: &str, dir: PathBuf) -> Result<Document> {
        Document::parse(
            text,
            dir.clone(),
            "devpage.yml".into(),
            ServedFolder::canonical(dir),
        )
    }

    #[test]
    fn parses_each_kind() {
        let dir = folder("kinds");
        let site = parse("kind: site\ntitle: Docs\nroot: README.md\n", dir.clone()).unwrap();
        assert_eq!(site.entry(), "/README.md");
        let doc = parse(
            "kind: doc\ntitle: Plan\nchapters: [README.md, guides/start.md]\npdf: {paper: Letter}\n",
            dir.clone(),
        )
        .unwrap();
        let Kind::Doc { chapters, pdf } = &doc.kind else {
            panic!("{doc:?}")
        };
        assert_eq!(chapters.len(), 2);
        assert_eq!(pdf.paper, Paper::Letter);
        assert_eq!(pdf.footer, "{page} / {pages}");
        assert_eq!(doc.entry(), "/devpage.yml");
        let slides = parse("kind: slides\ntitle: Talk\nslides: [README.md]\n", dir).unwrap();
        assert!(matches!(slides.kind, Kind::Slides { .. }));
    }

    #[test]
    fn rejects_bad_kinds_fields_and_missing_files() {
        let dir = folder("bad");
        for text in [
            "title: Docs\nroot: README.md\n",
            "kind: deck\ntitle: Docs\nroot: README.md\n",
            "kind: site\ntitle: Docs\nroot: nope.md\n",
            "kind: site\ntitle: Docs\nroot: README.md\ncolour: red\n",
            "kind: site\ntitle: Docs\nroot: README.md\nwatch: \"*.md\"\n",
            "kind: site\ntitle: Docs\nroot: README.md\nwatch: [\"a[b\"]\n",
            "kind: doc\ntitle: Docs\nroot: README.md\nchapters: [README.md]\n",
            "kind: doc\ntitle: Docs\nchapters: [missing.md]\n",
            "kind: doc\ntitle: Docs\nchapters: [../outside.md]\n",
        ] {
            assert!(parse(text, dir.clone()).is_err(), "{text}");
        }
    }

    #[test]
    fn reads_the_navbar_in_order_and_refuses_bad_shapes() {
        let dir = folder("navbar");
        let document = parse(
            "kind: site\ntitle: T\nroot: README.md\nsidebar: false\ntheme-selector: false\nnavbar:\n  Docs (book-open): README.md\n  Plain: README.md\n  More (layout-grid):\n    Talk (presentation): talk.yml\n  (github): https://github.com/x/y\n",
            dir.clone(),
        )
        .unwrap();
        assert!(!document.sidebar && !document.theme_selector);
        let link = |label: Option<&str>, icon: Option<&str>, target: &str| NavLink {
            label: label.map(String::from),
            icon: icon.map(String::from),
            target: target.into(),
        };
        assert_eq!(
            document.navbar,
            [
                NavItem::Link(link(Some("Docs"), Some("book-open"), "README.md")),
                NavItem::Link(link(Some("Plain"), None, "README.md")),
                NavItem::Menu {
                    label: Some("More".into()),
                    icon: Some("layout-grid".into()),
                    items: vec![link(Some("Talk"), Some("presentation"), "talk.yml")],
                },
                NavItem::Link(link(None, Some("github"), "https://github.com/x/y")),
            ]
        );
        // Defaults: a sidebar, a selector, no navbar; a doc and a deck may opt out too.
        let plain = parse("kind: site\ntitle: T\nroot: README.md\n", dir.clone()).unwrap();
        assert!(plain.sidebar && plain.theme_selector && plain.navbar.is_empty());
        assert!(
            !parse(
                "kind: doc\ntitle: T\nchapters: [README.md]\ntheme-selector: false\n",
                dir.clone()
            )
            .unwrap()
            .theme_selector
        );
        for text in [
            // two levels deep, an empty dropdown, a list instead of a map, no label or icon
            "kind: site\ntitle: T\nroot: README.md\nnavbar:\n  A:\n    B:\n      C: README.md\n",
            "kind: site\ntitle: T\nroot: README.md\nnavbar:\n  A: {}\n",
            "kind: site\ntitle: T\nroot: README.md\nnavbar:\n  - A: README.md\n",
            "kind: site\ntitle: T\nroot: README.md\nnavbar:\n  \"  \": README.md\n",
            "kind: site\ntitle: T\nroot: README.md\nnavbar:\n  A: 3\n",
            // a navbar and a sidebar belong to sites
            "kind: doc\ntitle: T\nchapters: [README.md]\nnavbar:\n  A: README.md\n",
            "kind: slides\ntitle: T\nslides: [README.md]\nsidebar: false\n",
        ] {
            assert!(parse(text, dir.clone()).is_err(), "{text}");
        }
    }

    #[test]
    fn a_markdown_file_is_a_doc_of_one_chapter() {
        let dir = folder("md");
        let doc = Document::load(
            &dir.join("guides/start.md"),
            &ServedFolder::new(&dir).unwrap(),
        )
        .unwrap();
        assert_eq!(doc.title, "Start");
        assert!(matches!(&doc.kind, Kind::Doc { chapters, .. } if chapters.len() == 1));
    }

    #[test]
    fn the_served_folder_is_the_boundary_and_addresses_start_there() {
        // No .git anywhere: the served folder is whatever devpage was given.
        let served = folder("served");
        std::fs::write(
            served.join("guides/devpage.yml"),
            "kind: site\ntitle: G\nroot: start.md\n",
        )
        .unwrap();
        let doc =
            Document::load(&served.join("guides"), &ServedFolder::new(&served).unwrap()).unwrap();
        assert_eq!(doc.served.path(), served);
        assert_eq!(doc.entry(), "/guides/start.md");
        assert!(
            doc.file("/README.md").is_some(),
            "files beside the docs are served"
        );
        assert!(doc.file("/../x").is_none());

        // --root moves the boundary: served from guides/, README.md is outside.
        let guides = served.join("guides");
        let narrow = Document::load(&guides, &ServedFolder::new(&guides).unwrap()).unwrap();
        assert_eq!(narrow.entry(), "/start.md");
        assert!(narrow.file("/../README.md").is_none());
    }

    #[test]
    fn a_document_outside_the_served_folder_is_refused() {
        let served = folder("outside");
        let error = Document::load(
            &served.join("README.md"),
            &ServedFolder::new(&served.join("guides")).unwrap(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("is outside the served folder"), "{error}");
    }

    #[test]
    fn a_site_watch_list_narrows_what_refreshes() {
        let dir = folder("watch");
        let all = parse("kind: site\ntitle: D\nroot: README.md\n", dir.clone()).unwrap();
        assert!(all.watches(&dir.join("logo.png")));
        let narrow = parse(
            "kind: site\ntitle: D\nroot: README.md\nwatch: [\"**/*.md\"]\n",
            dir.clone(),
        )
        .unwrap();
        assert!(narrow.watches(&dir.join("guides/start.md")));
        assert!(!narrow.watches(&dir.join("logo.png")));
    }

    #[test]
    fn only_yaml_with_a_devpage_kind_is_a_document_file() {
        let dir = folder("kind-of");
        std::fs::write(dir.join("site.yml"), "kind: site\ntitle: T\n").unwrap();
        std::fs::write(dir.join("deploy.yaml"), "kind: Deployment\n").unwrap();
        std::fs::write(dir.join("ci.yml"), "on: push\n").unwrap();
        assert_eq!(kind_of(&dir.join("site.yml")), Some(KindName::Site));
        assert_eq!(kind_of(&dir.join("deploy.yaml")), None);
        assert_eq!(kind_of(&dir.join("ci.yml")), None);
        assert_eq!(kind_of(&dir.join("README.md")), None);
    }

    #[test]
    fn a_deck_keeps_its_narration_and_each_slides_notes_and_delivery() {
        let dir = folder("notes");
        std::fs::write(
            dir.join("one.md"),
            "# One\n\n<Notes delivery=\"Warm, a smile in the voice.\">\n  Hello, everyone.\n</Notes>\n\n<Notes>second</Notes>\n",
        )
        .unwrap();
        std::fs::write(dir.join("two.md"), "# Two\n\n`<Notes>not these</Notes>`\n").unwrap();
        let deck = parse(
            "kind: slides\ntitle: Talk\naudience: Engineers.\ngoal: They try it.\ntone: Warm and brief.\npronounce: ['Say \"devpage\" as \"dev page\".']\nslides: [one.md, two.md]\n",
            dir.clone(),
        )
        .unwrap();
        let Kind::Slides { slides, narration } = &deck.kind else {
            panic!("{deck:?}")
        };
        assert_eq!(
            narration,
            &Narration {
                audience: Some("Engineers.".to_string()),
                goal: Some("They try it.".to_string()),
                tone: Some("Warm and brief.".to_string()),
                pronounce: vec!["Say \"devpage\" as \"dev page\".".to_string()],
            }
        );
        assert_eq!(slides[0].file, dir.join("one.md"));
        assert_eq!(slides[0].notes.as_deref(), Some("Hello, everyone."));
        assert_eq!(
            slides[0].delivery.as_deref(),
            Some("Warm, a smile in the voice.")
        );
        assert_eq!(slides[1].notes, None);
        let plain = parse("kind: slides\ntitle: T\nslides: [one.md]\n", dir.clone()).unwrap();
        assert!(
            matches!(&plain.kind, Kind::Slides { narration, .. } if *narration == Narration::default())
        );
        // The narration belongs to slides, and `context` is gone.
        for text in [
            "kind: doc\ntitle: T\nchapters: [README.md]\naudience: x\n",
            "kind: site\ntitle: T\nroot: README.md\ngoal: x\n",
            "kind: slides\ntitle: T\nslides: [one.md]\ncontext: x\n",
            "kind: slides\ntitle: T\nslides: [one.md]\npronounce: x\n",
        ] {
            assert!(parse(text, dir.clone()).is_err(), "{text}");
        }
    }
}
