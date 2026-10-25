//! A document: the YAML file that says what to show and what to watch.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;

/// The YAML as written. Paths in it are relative to the YAML file's folder.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    title: String,
    root: String,
    #[serde(default = "default_watch")]
    watch: Vec<String>,
}

fn default_watch() -> Vec<String> {
    vec!["**/*.md".into()]
}

/// A loaded document: its folder, the page it opens on, and the files whose
/// changes refresh open pages.
#[derive(Debug)]
pub struct Document {
    pub title: String,
    pub base: PathBuf,
    pub root: String,
    watch: GlobSet,
}

impl Document {
    /// Loads a document from its YAML file, or from `devpage.yml` in a folder.
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
        Self::parse(&text, base).with_context(|| format!("in {}", file.display()))
    }

    fn parse(text: &str, base: PathBuf) -> Result<Self> {
        let config: Config = serde_yaml::from_str(text)?;
        if !base.join(&config.root).is_file() {
            bail!("root page {} does not exist", config.root);
        }
        let mut watch = GlobSetBuilder::new();
        for pattern in &config.watch {
            watch.add(Glob::new(pattern).with_context(|| format!("watch pattern {pattern}"))?);
        }
        Ok(Self {
            title: config.title,
            base,
            root: config.root,
            watch: watch.build()?,
        })
    }

    /// Whether a change to this path (relative to the document's folder) should
    /// refresh open pages.
    pub fn watches(&self, relative: &Path) -> bool {
        self.watch.is_match(relative)
    }

    /// The file a URL path names, if it lies inside the document's folder.
    pub fn file(&self, url_path: &str) -> Option<PathBuf> {
        let candidate = self
            .base
            .join(url_path.trim_start_matches('/'))
            .canonicalize()
            .ok()?;
        candidate.starts_with(&self.base).then_some(candidate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("devpage-doc-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("guides")).unwrap();
        std::fs::write(dir.join("README.md"), "# Home").unwrap();
        std::fs::write(dir.join("guides/start.md"), "# Start").unwrap();
        dir.canonicalize().unwrap()
    }

    #[test]
    fn parses_config_and_defaults_watch_to_markdown() {
        let doc = Document::parse("title: Docs\nroot: README.md\n", folder()).unwrap();
        assert_eq!(doc.title, "Docs");
        assert!(doc.watches(Path::new("guides/start.md")));
        assert!(!doc.watches(Path::new("logo.png")));
    }

    #[test]
    fn rejects_missing_root_and_unknown_fields() {
        assert!(Document::parse("title: Docs\nroot: nope.md\n", folder()).is_err());
        assert!(Document::parse("title: Docs\nroot: README.md\ncolour: red\n", folder()).is_err());
    }

    #[test]
    fn loads_a_bare_file_name_from_the_current_folder() {
        let dir = folder();
        std::fs::write(dir.join("bare.yml"), "title: Docs\nroot: README.md\n").unwrap();
        let before = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        let loaded = Document::load(Path::new("bare.yml"));
        std::env::set_current_dir(before).unwrap();
        assert_eq!(loaded.unwrap().base, dir);
    }

    #[test]
    fn serves_only_files_inside_the_document_folder() {
        let doc = Document::parse("title: Docs\nroot: README.md\n", folder()).unwrap();
        assert!(doc.file("/guides/start.md").is_some());
        assert!(doc.file("/../../etc/passwd").is_none());
    }
}
