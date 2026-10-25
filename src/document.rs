//! A document: what a document file says to show — a site, a doc or slides —
//! or a lone Markdown file, which is a doc of one chapter.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use anyhow::{Context, Result, bail};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;

/// The YAML as written, tagged by `kind`. Paths are relative to its folder.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
enum Config {
    Site {
        title: String,
        root: String,
        watch: Option<Vec<String>>,
    },
    Doc {
        title: String,
        chapters: Vec<String>,
        #[serde(default)]
        pdf: Pdf,
    },
    Slides {
        title: String,
        slides: Vec<String>,
    },
}

/// A doc's settings on paper.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Pdf {
    #[serde(default)]
    pub paper: Paper,
    pub header: Option<String>,
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

fn default_footer() -> String {
    "{page} / {pages}".into()
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq)]
pub enum Paper {
    #[default]
    A4,
    Letter,
}

/// What a document is, with its parts resolved to files.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Site {
        root: String,
    },
    Doc {
        chapters: Vec<PathBuf>,
        pdf: Pdf,
    },
    Slides {
        slides: Vec<PathBuf>,
    },
    /// A `.design` file, which is the whole document.
    Design,
}

/// Which kind a document file names, read without loading the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindName {
    Site,
    Doc,
    Slides,
}

/// A loaded document: its title, its folder, what it is, and where it is served.
#[derive(Debug)]
pub struct Document {
    pub title: String,
    pub base: PathBuf,
    pub kind: Kind,
    /// The document file's name inside `base`.
    pub file_name: String,
    /// The served folder: where devpage was started, or `--root`. Addresses
    /// are paths in it, and nothing outside it is served.
    pub served: PathBuf,
    /// A site's `watch` list: when set, only matching files refresh pages.
    watch: Option<GlobSet>,
}

impl Document {
    /// Loads a document file, `devpage.yml` in a folder, a Markdown file or a
    /// design, which must lie inside the served folder `served`.
    pub fn load(path: &Path, served: &Path) -> Result<Self> {
        let file = if path.is_dir() {
            path.join("devpage.yml")
        } else {
            path.to_path_buf()
        };
        let text = std::fs::read_to_string(&file)
            .with_context(|| format!("reading {}", file.display()))?;
        let served = served
            .canonicalize()
            .with_context(|| format!("reading {}", served.display()))?;
        // A bare file name has an empty parent, which means the current folder.
        let parent = file.parent().filter(|p| !p.as_os_str().is_empty());
        let base = parent.unwrap_or(Path::new(".")).canonicalize()?;
        if !base.starts_with(&served) {
            bail!(
                "{} is outside the served folder {}",
                path.display(),
                served.display()
            );
        }
        let file_name = file
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if file.extension().is_some_and(|ext| ext == "design") {
            return Ok(Self {
                served,
                title: file
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                kind: Kind::Design,
                base,
                file_name,
                watch: None,
            });
        }
        if file.extension().is_some_and(|ext| ext == "md") {
            return Ok(Self {
                served,
                title: crate::render::title(&text, &file),
                kind: Kind::Doc {
                    chapters: vec![base.join(&file_name)],
                    pdf: Pdf::default(),
                },
                base,
                file_name,
                watch: None,
            });
        }
        Self::parse(&text, base, file_name, served)
            .with_context(|| format!("in {}", file.display()))
    }

    fn parse(text: &str, base: PathBuf, file_name: String, served: PathBuf) -> Result<Self> {
        // A part must be an existing file inside the served folder.
        let existing = |field: &str, path: &str| -> Result<PathBuf> {
            match base.join(path).canonicalize() {
                Ok(file) if file.is_file() && file.starts_with(&served) => Ok(file),
                _ => bail!("{field} {path} does not exist"),
            }
        };
        let mut watch = None;
        let (title, kind) = match serde_yaml::from_str(text)? {
            Config::Site {
                title,
                root,
                watch: patterns,
            } => {
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
                }
                (title, Kind::Site { root })
            }
            Config::Doc {
                title,
                chapters,
                pdf,
            } => {
                if chapters.is_empty() {
                    bail!("chapters lists no files");
                }
                let chapters = chapters
                    .iter()
                    .map(|chapter| existing("chapter", chapter))
                    .collect::<Result<_>>()?;
                (title, Kind::Doc { chapters, pdf })
            }
            Config::Slides { title, slides } => {
                if slides.is_empty() {
                    bail!("slides lists no files");
                }
                let slides = slides
                    .iter()
                    .map(|slide| existing("slide", slide))
                    .collect::<Result<_>>()?;
                (title, Kind::Slides { slides })
            }
        };
        Ok(Self {
            title,
            base,
            kind,
            file_name,
            served,
            watch,
        })
    }

    /// Where `/` leads: a site's root page, or the document file itself.
    pub fn entry(&self) -> String {
        match &self.kind {
            Kind::Site { root } => self.url(&self.base.join(root)),
            _ => self.url(&self.path()),
        }
    }

    /// The document file itself.
    pub fn path(&self) -> PathBuf {
        self.base.join(&self.file_name)
    }

    /// A file's address: its path from the top of the served folder.
    pub fn url(&self, file: &Path) -> String {
        format!(
            "/{}",
            file.strip_prefix(&self.served).unwrap_or(file).display()
        )
    }

    /// The file a URL path names, if it lies inside the served folder.
    pub fn file(&self, url_path: &str) -> Option<PathBuf> {
        let candidate = self
            .served
            .join(url_path.trim_start_matches('/'))
            .canonicalize()
            .ok()?;
        candidate.starts_with(&self.served).then_some(candidate)
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
}

/// The document devpage was started with, re-read whenever its file changes,
/// so `/` and the watcher follow the file without a restart. While the file
/// is invalid, the last valid document stays in use and the error is kept to
/// show in its place.
pub struct Entry {
    /// The entry's file: a document file, or a Markdown file served as a doc.
    file: PathBuf,
    state: RwLock<(Arc<Document>, Option<String>)>,
}

impl Entry {
    pub fn new(document: Arc<Document>) -> Self {
        Self {
            file: document.path(),
            state: RwLock::new((document, None)),
        }
    }

    /// The entry document as last read without error.
    pub fn document(&self) -> Arc<Document> {
        self.state.read().unwrap().0.clone()
    }

    /// Why the entry's file can't be read as a document, if it can't now.
    pub fn error(&self) -> Option<String> {
        self.state.read().unwrap().1.clone()
    }

    pub fn file(&self) -> &Path {
        &self.file
    }

    /// Reads the entry's file again: a valid file replaces the document, an
    /// invalid one keeps it and records the error.
    pub fn reload(&self) {
        let served = self.document().served.clone();
        let mut state = self.state.write().unwrap();
        match Document::load(&self.file, &served) {
            Ok(document) => *state = (Arc::new(document), None),
            Err(error) => state.1 = Some(format!("{error:#}")),
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("devpage-doc-{name}-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("guides")).unwrap();
        std::fs::write(dir.join("README.md"), "# Home").unwrap();
        std::fs::write(dir.join("guides/start.md"), "# Start").unwrap();
        dir.canonicalize().unwrap()
    }

    fn parse(text: &str, dir: PathBuf) -> Result<Document> {
        Document::parse(text, dir.clone(), "devpage.yml".into(), dir)
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
    fn a_markdown_file_is_a_doc_of_one_chapter() {
        let dir = folder("md");
        let doc = Document::load(&dir.join("guides/start.md"), &dir).unwrap();
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
        let doc = Document::load(&served.join("guides"), &served).unwrap();
        assert_eq!(doc.served, served);
        assert_eq!(doc.entry(), "/guides/start.md");
        assert!(
            doc.file("/README.md").is_some(),
            "files beside the docs are served"
        );
        assert!(doc.file("/../x").is_none());

        // --root moves the boundary: served from guides/, README.md is outside.
        let guides = served.join("guides");
        let narrow = Document::load(&guides, &guides).unwrap();
        assert_eq!(narrow.entry(), "/start.md");
        assert!(narrow.file("/../README.md").is_none());
    }

    #[test]
    fn a_document_outside_the_served_folder_is_refused() {
        let served = folder("outside");
        let error = Document::load(&served.join("README.md"), &served.join("guides"))
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
}
