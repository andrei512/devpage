//! The document index: which files of the served folder are document files
//! and of what kind, and which paths its `.gitignore` files exclude. Built
//! once when the folder is opened and kept current by [`Folder::changed`](super::Folder::changed),
//! so answering a request never scans folders.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use ignore::WalkBuilder;
use ignore::gitignore::Gitignore;

use super::ServedFolder;
use crate::document::{KindName, kind_of};

pub(super) struct Index {
    served: ServedFolder,
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
    /// Indexes the served folder.
    pub(super) fn build(served: &ServedFolder) -> Self {
        let index = Self {
            served: served.clone(),
            inner: RwLock::new(Inner::default()),
        };
        index.scan(served.path());
        index
    }

    /// The served folder.
    pub(super) fn served(&self) -> &ServedFolder {
        &self.served
    }

    /// Whether `path` is out of sight: inside `.git` or `.devpage`, or
    /// excluded by a `.gitignore` in the served folder.
    pub(super) fn ignored(&self, path: &Path, is_dir: bool) -> bool {
        let Some(relative) = self.served.relative(path) else {
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
            if folder == self.served.path() {
                break;
            }
        }
        false
    }

    /// The nearest site whose document file's folder holds `page`.
    pub(super) fn site_for(&self, page: &Path) -> Option<PathBuf> {
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
    pub(super) fn kind(&self, file: &Path) -> Option<KindName> {
        self.read().documents.get(file).copied()
    }

    /// Every indexed document file.
    pub(super) fn documents(&self) -> Vec<PathBuf> {
        self.read().documents.keys().cloned().collect()
    }

    /// Brings the index up to date after `path` changed, appeared or vanished.
    pub(super) fn changed(&self, path: &Path) {
        if !self.served.contains(path) {
            return;
        }
        let is_gitignore = path.file_name().is_some_and(|name| name == ".gitignore");
        if is_gitignore {
            // What is excluded changed: index the folder it governs afresh.
            let folder = path.parent().unwrap_or(self.served.path()).to_path_buf();
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
