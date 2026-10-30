//! The served folder and what is in it: the boundary every path and URL is
//! judged against, its document files by kind, its `.gitignore` rules, the
//! entry document devpage was started with, which document owns a page or a
//! widgets folder, and what a change to its files means — for open pages and
//! for the folders to watch. Documents are loaded per call: the folder keeps
//! the document index and the entry, and no other Document.

mod index;
mod served;
mod sight;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use anyhow::Result;

use crate::changes::Change;
use crate::document::{Document, KindName};
use index::Index;

pub(crate) use served::lexical;
#[cfg(any(test, feature = "testing"))]
pub use served::symlink;
pub use served::{ServedFolder, slashed};
pub use sight::WatchSet;

/// A served folder opened on its entry document.
pub struct Folder {
    index: Index,
    entry: Entry,
}

/// The document that owns a page: the nearest site whose folder holds it,
/// as its file reads now — or why it can't be read.
pub struct Owner {
    /// The site's document file.
    pub file: PathBuf,
    pub document: Result<Document>,
}

/// What a burst of changed files means, once the folder has heard of them.
#[derive(Debug, Default, PartialEq)]
pub struct Changed {
    /// What open pages should hear: at most one [`Change::Refresh`].
    pub changes: Vec<Change>,
    /// Whether the folders to watch may differ now — a `.gitignore`, the
    /// entry's file or a site's document file changed, or a document file
    /// went away — so [`Folder::watched`] should be asked again.
    pub reshaped: bool,
}

impl Folder {
    /// The served folder `entry` was loaded from, indexed, with `entry` as
    /// its entry document.
    pub fn new(entry: Arc<Document>) -> Self {
        Self {
            index: Index::build(&entry.served),
            entry: Entry::new(entry),
        }
    }

    /// The served folder.
    pub fn served(&self) -> &ServedFolder {
        self.index.served()
    }

    /// The entry document as last read without error.
    pub fn entry(&self) -> Arc<Document> {
        self.entry.document()
    }

    /// Why the entry's file can't be read as a document, if it can't now.
    pub fn entry_error(&self) -> Option<String> {
        self.entry.error()
    }

    /// The entry's file: a document file, or a Markdown file served as a doc.
    pub fn entry_file(&self) -> &Path {
        &self.entry.file
    }

    /// The document file of the nearest site whose folder holds `page`,
    /// unloaded; `None` when no site does.
    pub fn site(&self, page: &Path) -> Option<PathBuf> {
        self.index.site_for(page)
    }

    /// The nearest site whose folder holds `page`, loaded now; `None` when
    /// no site does.
    pub fn owner(&self, page: &Path) -> Option<Owner> {
        let file = self.site(page)?;
        let document = Document::load(&file, self.served());
        Some(Owner { file, document })
    }

    /// The document whose widgets folder is `folder`: the entry document
    /// when it names it, or else the first indexed document that does.
    pub fn widgets_owner(&self, folder: &Path) -> Option<Arc<Document>> {
        let entry = self.entry();
        if entry.widgets.as_deref() == Some(folder) {
            return Some(entry);
        }
        self.documents()
            .into_iter()
            .filter_map(|file| Document::load(&file, self.served()).ok())
            .find(|document| document.widgets.as_deref() == Some(folder))
            .map(Arc::new)
    }

    /// The folders to watch, so that nothing out of sight is.
    pub fn watched(&self) -> WatchSet {
        WatchSet::plan(self)
    }

    /// `paths` — canonical, or as reported when gone — changed, appeared or
    /// vanished: the index and the entry follow them, and what that means
    /// comes back. A path outside the folder or under `.devpage/` is not the
    /// folder's. The entry's own file refreshes open pages whatever `watch`
    /// says; any other path in sight refreshes them as its site's `watch`
    /// list decides.
    pub fn changed(&self, paths: &[PathBuf]) -> Changed {
        let mut refresh = false;
        let mut reshaped = false;
        for path in paths {
            let Some(relative) = self.served().relative(path) else {
                continue;
            };
            if relative.starts_with(".devpage") {
                continue;
            }
            let was_site = self.kind(path) == Some(KindName::Site);
            self.index.changed(path);
            let document_file = path
                .extension()
                .is_some_and(|ext| ext == "yml" || ext == "yaml");
            reshaped |= path.file_name().is_some_and(|name| name == ".gitignore")
                || path == self.entry_file()
                || was_site
                || self.kind(path) == Some(KindName::Site)
                || (document_file && !path.exists());
            if path == self.entry_file() {
                self.entry.reload(self.served());
                refresh = true;
            } else if !self.ignored(path, path.is_dir()) && self.refreshes(path) {
                refresh = true;
            }
        }
        Changed {
            changes: if refresh {
                vec![Change::Refresh]
            } else {
                Vec::new()
            },
            reshaped,
        }
    }

    /// Whether `path` is out of sight: inside `.git` or `.devpage`, or
    /// excluded by a `.gitignore` in the served folder.
    pub(crate) fn ignored(&self, path: &Path, is_dir: bool) -> bool {
        self.index.ignored(path, is_dir)
    }

    /// The kind of an indexed document file.
    fn kind(&self, file: &Path) -> Option<KindName> {
        self.index.kind(file)
    }

    /// Every indexed document file.
    fn documents(&self) -> Vec<PathBuf> {
        self.index.documents()
    }
}

/// The document devpage was started with, re-read whenever its file changes,
/// so `/` and the watcher follow the file without a restart. While the file
/// is invalid, the last valid document stays in use and the error is kept to
/// show in its place.
struct Entry {
    file: PathBuf,
    state: RwLock<(Arc<Document>, Option<String>)>,
}

impl Entry {
    fn new(document: Arc<Document>) -> Self {
        Self {
            file: document.path(),
            state: RwLock::new((document, None)),
        }
    }

    fn document(&self) -> Arc<Document> {
        self.read().0.clone()
    }

    fn error(&self) -> Option<String> {
        self.read().1.clone()
    }

    /// Reads the entry's file again: a valid file replaces the document, an
    /// invalid one keeps it and records the error.
    fn reload(&self, served: &ServedFolder) {
        let loaded = Document::load(&self.file, served);
        let mut state = self
            .state
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match loaded {
            Ok(document) => *state = (Arc::new(document), None),
            Err(error) => state.1 = Some(format!("{error:#}")),
        }
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, (Arc<Document>, Option<String>)> {
        self.state
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
