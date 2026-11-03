//! The watcher everywhere but macOS, Linux and Windows: `notify`'s recommended
//! watcher, recursive on the whole served folder, since it can't leave a
//! subtree out. Sifting drops what is out of sight.

use std::path::{Path, PathBuf};

use anyhow::Result;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use super::{Handler, WatchSet};

/// The platform's watcher, on the served folder. Stops when dropped.
pub struct Platform {
    /// `notify`'s recommended watcher, recursive on the served folder, held so
    /// it runs until dropped.
    _watcher: RecommendedWatcher,
}

impl Platform {
    /// A watcher that hands `handler` every event in `served`, at any depth.
    pub fn new(served: &Path, handler: Handler) -> Result<Self> {
        let mut watcher = notify::recommended_watcher(move |event| handler(event))?;
        watcher.watch(served, RecursiveMode::Recursive)?;
        Ok(Self { _watcher: watcher })
    }

    /// The whole served folder is watched whatever the watch set says.
    pub fn apply(&mut self, _set: WatchSet) -> Vec<(PathBuf, bool)> {
        Vec::new()
    }

    /// Whether changes in `folder` are already reported: always, as the whole
    /// served folder is watched.
    pub fn covers(&self, _folder: &Path) -> bool {
        true
    }

    /// Whether a watched folder is `path` or inside it; always `false`, since no
    /// change can reach past a watch on the whole served folder.
    pub fn watches_within(&self, _path: &Path) -> bool {
        false
    }

    #[cfg(test)]
    pub fn live(&self, _folder: &Path) -> bool {
        true
    }
}
