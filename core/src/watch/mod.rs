//! Local mode's one ordered response to saved files. The binary's watcher
//! turns the platform's file events into bursts of changed paths and hands
//! each burst here; everything that follows from it is decided here, in one
//! order: the folder follows the paths, the design store and the widget
//! modules forget what they read of them, open pages hear what changed —
//! a page's comments for each thread another hand changed, then at most one
//! refresh — and last the search index reads them again, so indexing text
//! never holds up a refresh. Server mode has none of this: a published
//! version never changes.

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::Arc;

use crate::changes::Publish;
use crate::comments::Comments;
use crate::designs::Designs;
use crate::folder::Folder;
use crate::search::Search;
use crate::widgets::Modules;

/// Who hears about the files that change in the served folder, and the bus
/// open pages hear it on.
pub struct Listeners {
    folder: Arc<Folder>,
    designs: Designs,
    modules: Modules,
    /// The comments the server writes through, which know the thread files
    /// they wrote — and announced — themselves; `None` with comments off.
    comments: Option<Comments>,
    changes: Arc<dyn Publish>,
    /// The search index, when this devpage answers searches.
    search: Option<Arc<Search>>,
}

impl Listeners {
    /// The listeners of `folder`: each is the same value the server answers
    /// from, so what it forgets the server reads again.
    pub fn new(
        folder: Arc<Folder>,
        designs: Designs,
        modules: Modules,
        comments: Option<Comments>,
        changes: Arc<dyn Publish>,
        search: Option<Arc<Search>>,
    ) -> Self {
        Self {
            folder,
            designs,
            modules,
            comments,
            changes,
            search,
        }
    }

    /// The served folder, which says what can matter and what to watch.
    pub fn folder(&self) -> &Folder {
        &self.folder
    }

    /// `paths` — canonical, or as reported when gone — changed, appeared or
    /// vanished. The folder follows them; the design store and the widget
    /// modules forget every one in the served folder and outside
    /// `.devpage/`, whatever `watch` says, since any file may be part of a
    /// design; open pages hear a page's comments for each thread another
    /// hand changed — never the server's own write, announced as it landed,
    /// and none with comments off — then what the folder says; then the
    /// search index reads them again.
    /// Says whether the folders to watch may differ now.
    pub fn changed(&self, paths: &[PathBuf]) -> bool {
        let mut heard = self
            .comments
            .as_ref()
            .map(|comments| comments.heard(paths))
            .unwrap_or_default();
        let folder = self.folder.changed(paths);
        let files: Vec<PathBuf> = paths
            .iter()
            .filter(|path| {
                self.folder
                    .served()
                    .relative(path)
                    .is_some_and(|relative| !relative.starts_with(".devpage"))
            })
            .cloned()
            .collect();
        for file in &files {
            self.designs.changed(file);
            self.modules.forget(file);
        }
        heard.extend(folder.changes);
        for change in heard {
            self.changes.publish(change);
        }
        if let Some(search) = &self.search {
            search.changed(&self.folder, &files);
        }
        folder.reshaped
    }
}
