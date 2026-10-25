//! A document: what a document file says to show — a site, a doc or slides —
//! or a lone Markdown file, which is a doc of one chapter.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// The YAML as written, tagged by `kind`. Paths are relative to its folder.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
enum Config {
    Site {
        title: String,
        root: String,
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
    Site { root: String },
    Doc { chapters: Vec<PathBuf>, pdf: Pdf },
    Slides { slides: Vec<PathBuf> },
}

/// A loaded document: its title, its folder, what it is, and where it is served.
#[derive(Debug)]
pub struct Document {
    pub title: String,
    pub base: PathBuf,
    pub kind: Kind,
    /// The document file's name inside `base`.
    pub file_name: String,
    /// The repository: the git working tree holding the document, or its
    /// folder outside git. Nothing outside it is served.
    pub repo: PathBuf,
}

impl Document {
    /// Loads a document file, `devpage.yml` in a folder, or a Markdown file.
    pub fn load(path: &Path) -> Result<Self> {
        let file = if path.is_dir() {
            path.join("devpage.yml")
        } else {
            path.to_path_buf()
        };
        let text = std::fs::read_to_string(&file)
            .with_context(|| format!("reading {}", file.display()))?;
        // A bare file name has an empty parent, which means the current folder.
        let parent = file.parent().filter(|p| !p.as_os_str().is_empty());
        let base = parent.unwrap_or(Path::new(".")).canonicalize()?;
        let file_name = file
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let repo = repository(&base);
        if file.extension().is_some_and(|ext| ext == "md") {
            return Ok(Self {
                repo,
                title: crate::render::title(&text, &file),
                kind: Kind::Doc {
                    chapters: vec![base.join(&file_name)],
                    pdf: Pdf::default(),
                },
                base,
                file_name,
            });
        }
        Self::parse(&text, base, file_name, repo).with_context(|| format!("in {}", file.display()))
    }

    fn parse(text: &str, base: PathBuf, file_name: String, repo: PathBuf) -> Result<Self> {
        let existing = |field: &str, path: &str| -> Result<PathBuf> {
            let file = base.join(path);
            if !file.is_file() {
                bail!("{field} {path} does not exist");
            }
            Ok(file)
        };
        let (title, kind) = match serde_yaml::from_str(text)? {
            Config::Site { title, root } => {
                existing("root", &root)?;
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
            repo,
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

    /// A file's address: its path from the top of the repository.
    pub fn url(&self, file: &Path) -> String {
        format!(
            "/{}",
            file.strip_prefix(&self.repo).unwrap_or(file).display()
        )
    }

    /// The file a URL path names, if it lies inside the repository.
    pub fn file(&self, url_path: &str) -> Option<PathBuf> {
        let candidate = self
            .repo
            .join(url_path.trim_start_matches('/'))
            .canonicalize()
            .ok()?;
        candidate.starts_with(&self.repo).then_some(candidate)
    }
}

/// The git working tree holding `folder`, or `folder` itself outside git.
fn repository(folder: &Path) -> PathBuf {
    folder
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .unwrap_or(folder)
        .to_path_buf()
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
            "kind: doc\ntitle: Docs\nroot: README.md\nchapters: [README.md]\n",
            "kind: doc\ntitle: Docs\nchapters: [missing.md]\n",
        ] {
            assert!(parse(text, dir.clone()).is_err(), "{text}");
        }
    }

    #[test]
    fn a_markdown_file_is_a_doc_of_one_chapter() {
        let dir = folder("md");
        let doc = Document::load(&dir.join("guides/start.md")).unwrap();
        assert_eq!(doc.title, "Start");
        assert!(matches!(&doc.kind, Kind::Doc { chapters, .. } if chapters.len() == 1));
    }

    #[test]
    fn the_repository_is_the_git_working_tree_and_addresses_start_there() {
        let repo = folder("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(
            repo.join("guides/devpage.yml"),
            "kind: site\ntitle: G\nroot: start.md\n",
        )
        .unwrap();
        let doc = Document::load(&repo.join("guides")).unwrap();
        assert_eq!(doc.repo, repo);
        assert_eq!(doc.entry(), "/guides/start.md");
        assert!(
            doc.file("/README.md").is_some(),
            "files beside the docs are served"
        );
        assert!(doc.file("/../x").is_none());
    }

    #[test]
    fn serves_only_files_inside_the_document_folder() {
        let doc = parse(
            "kind: site\ntitle: Docs\nroot: README.md\n",
            folder("inside"),
        )
        .unwrap();
        assert!(doc.file("/guides/start.md").is_some());
        assert!(doc.file("/../../etc/passwd").is_none());
    }
}
