//! The served folder's index: which files are document files and of what
//! kind, and which paths its `.gitignore` files exclude. Built once at start
//! and kept current by the watcher, so serving never scans folders.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use ignore::WalkBuilder;
use ignore::gitignore::Gitignore;

use crate::document::{KindName, kind_of};

pub struct Index {
    served: PathBuf,
    inner: RwLock<Inner>,
}

#[derive(Default)]
struct Inner {
    /// Every document file, by its canonical path.
    documents: BTreeMap<PathBuf, KindName>,
    /// Each `.gitignore` in the served folder, by the folder holding it.
    ignores: BTreeMap<PathBuf, Gitignore>,
}

impl Index {
    /// Indexes the served folder, which must be canonical.
    pub fn build(served: &Path) -> Self {
        let index = Self {
            served: served.to_path_buf(),
            inner: RwLock::new(Inner::default()),
        };
        index.scan(served);
        index
    }

    /// Whether `path` is out of sight: inside `.git` or `.devpage`, or
    /// excluded by a `.gitignore` in the served folder.
    pub fn ignored(&self, path: &Path, is_dir: bool) -> bool {
        let Ok(relative) = path.strip_prefix(&self.served) else {
            return true;
        };
        if relative
            .components()
            .any(|part| part.as_os_str() == ".git" || part.as_os_str() == ".devpage")
        {
            return true;
        }
        let inner = self.read();
        // The deepest `.gitignore` with an opinion decides, as in git.
        for folder in path.ancestors().skip(1) {
            if let Some(gitignore) = inner.ignores.get(folder) {
                let matched = gitignore.matched_path_or_any_parents(path, is_dir);
                if matched.is_ignore() {
                    return true;
                }
                if matched.is_whitelist() {
                    return false;
                }
            }
            if folder == self.served {
                break;
            }
        }
        false
    }

    /// The nearest site whose document file's folder holds `page`.
    pub fn site_for(&self, page: &Path) -> Option<PathBuf> {
        let inner = self.read();
        inner
            .documents
            .iter()
            .filter(|(file, kind)| {
                **kind == KindName::Site
                    && file.parent().is_some_and(|folder| page.starts_with(folder))
            })
            .max_by_key(|(file, _)| {
                (
                    file.components().count(),
                    std::cmp::Reverse((*file).clone()),
                )
            })
            .map(|(file, _)| file.clone())
    }

    /// The kind of an indexed document file.
    #[cfg(test)]
    pub fn kind(&self, file: &Path) -> Option<KindName> {
        self.read().documents.get(file).copied()
    }

    /// Brings the index up to date after `path` changed, appeared or vanished.
    pub fn changed(&self, path: &Path) {
        if !path.starts_with(&self.served) {
            return;
        }
        let is_gitignore = path.file_name().is_some_and(|name| name == ".gitignore");
        if is_gitignore {
            // What is excluded changed: index the folder it governs afresh.
            let folder = path.parent().unwrap_or(&self.served).to_path_buf();
            {
                let mut inner = self.write();
                inner.ignores.retain(|dir, _| !dir.starts_with(&folder));
                inner.documents.retain(|file, _| !file.starts_with(&folder));
            }
            self.scan(&folder);
            return;
        }
        if path.is_dir() {
            if !self.ignored(path, true) {
                self.scan(path);
            }
            return;
        }
        let mut inner = self.write();
        if !path.exists() {
            // A file, or a whole folder, went away.
            inner.documents.retain(|file, _| !file.starts_with(path));
            inner.ignores.retain(|dir, _| !dir.starts_with(path));
            return;
        }
        drop(inner);
        let kind = (!self.ignored(path, false))
            .then(|| kind_of(path))
            .flatten();
        let mut inner = self.write();
        match kind {
            Some(kind) => inner.documents.insert(path.to_path_buf(), kind),
            None => inner.documents.remove(path),
        };
    }

    /// Walks `folder`, honoring every `.gitignore` in the served folder, and
    /// records the document files and `.gitignore` files it finds.
    fn scan(&self, folder: &Path) {
        let walk = WalkBuilder::new(folder)
            .hidden(false)
            .parents(false)
            .ignore(false)
            .git_global(false)
            .git_exclude(false)
            .require_git(false)
            .filter_entry(|entry| entry.file_name() != ".git" && entry.file_name() != ".devpage")
            .build();
        let mut documents = Vec::new();
        let mut ignores = Vec::new();
        for entry in walk.flatten() {
            let path = entry.path();
            if entry.file_type().is_some_and(|kind| kind.is_file()) {
                if entry.file_name() == ".gitignore" {
                    let (gitignore, _) = Gitignore::new(path);
                    ignores.push((path.parent().unwrap_or(folder).to_path_buf(), gitignore));
                } else if let Some(kind) = kind_of(path) {
                    documents.push((path.to_path_buf(), kind));
                }
            }
        }
        // The walk honors `.gitignore` files under `folder`; those above it
        // still apply when only part of the served folder is scanned again.
        let documents: Vec<_> = documents
            .into_iter()
            .filter(|(file, _)| !self.ignored(file, false))
            .collect();
        let mut inner = self.write();
        inner.documents.extend(documents);
        inner.ignores.extend(ignores);
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Inner> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, Inner> {
        self.inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn served(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("devpage-index-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("guides/deep")).unwrap();
        std::fs::create_dir_all(dir.join("target/doc")).unwrap();
        std::fs::write(dir.join("README.md"), "# Home").unwrap();
        std::fs::write(
            dir.join("site.yml"),
            "kind: site\ntitle: Outer\nroot: README.md\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("guides/inner.yml"),
            "kind: site\ntitle: Inner\nroot: a.md\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("guides/talk.yml"),
            "kind: slides\ntitle: T\nslides: [a.md]\n",
        )
        .unwrap();
        std::fs::write(dir.join("guides/a.md"), "# A").unwrap();
        std::fs::write(dir.join("guides/deep/b.md"), "# B").unwrap();
        std::fs::write(dir.join("deploy.yaml"), "kind: Deployment\n").unwrap();
        std::fs::write(dir.join(".gitignore"), "/target/\n").unwrap();
        std::fs::write(
            dir.join("target/doc/devpage.yml"),
            "kind: site\ntitle: X\nroot: a\n",
        )
        .unwrap();
        std::fs::write(dir.join("guides/.gitignore"), "*.tmp\n").unwrap();
        dir.canonicalize().unwrap()
    }

    #[test]
    fn indexes_document_files_outside_what_gitignore_excludes() {
        let dir = served("build");
        let index = Index::build(&dir);
        assert_eq!(index.kind(&dir.join("site.yml")), Some(KindName::Site));
        assert_eq!(
            index.kind(&dir.join("guides/talk.yml")),
            Some(KindName::Slides)
        );
        assert_eq!(
            index.kind(&dir.join("deploy.yaml")),
            None,
            "not devpage's kind"
        );
        assert_eq!(
            index.kind(&dir.join("target/doc/devpage.yml")),
            None,
            "ignored"
        );
    }

    #[test]
    fn a_page_belongs_to_the_nearest_site_above_it() {
        let dir = served("nearest");
        let index = Index::build(&dir);
        assert_eq!(
            index.site_for(&dir.join("guides/deep/b.md")),
            Some(dir.join("guides/inner.yml"))
        );
        assert_eq!(
            index.site_for(&dir.join("README.md")),
            Some(dir.join("site.yml"))
        );
    }

    #[test]
    fn nested_gitignores_and_git_folders_are_out_of_sight() {
        let dir = served("ignored");
        let index = Index::build(&dir);
        assert!(index.ignored(&dir.join("target/doc/x.md"), false));
        assert!(
            index.ignored(&dir.join("guides/notes.tmp"), false),
            "nested .gitignore"
        );
        assert!(
            !index.ignored(&dir.join("notes.tmp"), false),
            "only below guides/"
        );
        assert!(index.ignored(&dir.join(".git/HEAD"), false));
        assert!(index.ignored(&dir.join(".devpage/comments/a.md/x.yml"), false));
        assert!(!index.ignored(&dir.join("guides/a.md"), false));
    }

    #[test]
    fn changes_keep_it_current() {
        let dir = served("changes");
        let index = Index::build(&dir);
        let deck = dir.join("guides/deep/deck.yml");
        std::fs::write(&deck, "kind: slides\ntitle: D\nslides: [b.md]\n").unwrap();
        index.changed(&deck);
        assert_eq!(index.kind(&deck), Some(KindName::Slides));
        std::fs::write(
            dir.join("guides/deep/site.yml"),
            "kind: site\ntitle: S\nroot: b.md\n",
        )
        .unwrap();
        index.changed(&dir.join("guides/deep/site.yml"));
        assert_eq!(
            index.site_for(&dir.join("guides/deep/b.md")),
            Some(dir.join("guides/deep/site.yml"))
        );
        std::fs::remove_file(&deck).unwrap();
        index.changed(&deck);
        assert_eq!(index.kind(&deck), None);
        std::fs::write(dir.join(".gitignore"), "/target/\n/guides/deep/\n").unwrap();
        index.changed(&dir.join(".gitignore"));
        assert!(index.ignored(&dir.join("guides/deep/b.md"), false));
        assert_eq!(
            index.site_for(&dir.join("guides/deep/b.md")),
            Some(dir.join("guides/inner.yml"))
        );
    }
}
