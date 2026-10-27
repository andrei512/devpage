//! The Linux watcher: `notify`'s inotify watcher, non-recursive on every
//! folder in sight. inotify watches a folder at a time anyway, so leaving
//! the out-of-sight ones out means they get no inotify watch at all.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::Result;
use notify::{INotifyWatcher, RecursiveMode, Watcher};

use super::{Handler, WatchSet};

/// The platform's watcher over a watch set. Stops when dropped.
pub struct Platform {
    watcher: INotifyWatcher,
    /// Every folder watched: the shallow ones and every folder of the deep ones.
    folders: BTreeSet<PathBuf>,
}

impl Platform {
    pub fn new(_served: &Path, handler: Handler) -> Result<Self> {
        let watcher = INotifyWatcher::new(move |event| handler(event), notify::Config::default())?;
        Ok(Self {
            watcher,
            folders: BTreeSet::new(),
        })
    }

    /// Watches `set` in place of the last one: only folders that changed are
    /// added or removed. Returns the folders newly reported on, each for
    /// what is directly in it.
    pub fn apply(&mut self, set: WatchSet) -> Vec<(PathBuf, bool)> {
        let mut folders = set.shallow;
        for deep in &set.deep {
            every_folder(deep, &mut folders);
        }
        for gone in self.folders.difference(&folders) {
            let _ = self.watcher.unwatch(gone);
        }
        let mut added = Vec::new();
        for folder in folders.difference(&self.folders) {
            if self
                .watcher
                .watch(folder, RecursiveMode::NonRecursive)
                .is_ok()
            {
                added.push((folder.clone(), false));
            }
        }
        self.folders = folders;
        added
    }

    /// Whether changes in `folder` are already reported.
    pub fn covers(&self, folder: &Path) -> bool {
        self.folders.contains(folder)
    }

    /// Whether a watched folder is `path` or inside it.
    pub fn watches_within(&self, path: &Path) -> bool {
        self.folders.iter().any(|folder| folder.starts_with(path))
    }
}

/// `folder` and every folder under it, not following symbolic links.
fn every_folder(folder: &Path, folders: &mut BTreeSet<PathBuf>) {
    folders.insert(folder.to_path_buf());
    for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            every_folder(&entry.path(), folders);
        }
    }
}
