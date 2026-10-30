//! The Linux and Windows watcher: `notify`'s platform watcher — inotify, or
//! ReadDirectoryChangesW — non-recursive on every folder in sight. Both
//! watch a folder at a time, so the out-of-sight ones get no watch at all.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::Result;
use notify::event::{EventKind, ModifyKind};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use super::{Handler, WatchSet};

/// The platform's watcher over a watch set. Stops when dropped.
pub struct Platform {
    watcher: RecommendedWatcher,
    /// Every folder watched now: the shallow ones and every folder of the
    /// deep ones, less any deleted or renamed since its watch was added — a
    /// deleted folder's watch is dropped, and a folder made again with its
    /// name has none.
    folders: Arc<Mutex<BTreeSet<PathBuf>>>,
}

impl Platform {
    pub fn new(_served: &Path, handler: Handler) -> Result<Self> {
        let folders = Arc::new(Mutex::new(BTreeSet::new()));
        let watched = folders.clone();
        let watcher = RecommendedWatcher::new(
            move |event: notify::Result<notify::Event>| {
                #[cfg(windows)]
                let Some(event) = classified(event, &watched) else {
                    return;
                };
                if let Ok(event) = &event {
                    lost(&watched, event);
                }
                handler(event)
            },
            notify::Config::default(),
        )?;
        Ok(Self { watcher, folders })
    }

    /// Watches `set` in place of the last one: folders no longer in it stop
    /// being watched, and folders in it without a watch now — new, or
    /// deleted and made again — start. Returns the folders newly reported
    /// on, each for what is directly in it.
    pub fn apply(&mut self, set: WatchSet) -> Vec<(PathBuf, bool)> {
        let mut planned = set.shallow;
        for deep in &set.deep {
            every_folder(deep, &mut planned);
        }
        let watched = lock(&self.folders).clone();
        for gone in watched.difference(&planned) {
            let _ = self.watcher.unwatch(gone);
        }
        let mut added = Vec::new();
        for folder in planned.difference(&watched) {
            if self
                .watcher
                .watch(folder, RecursiveMode::NonRecursive)
                .is_ok()
            {
                added.push(folder.clone());
            }
        }
        let mut folders = lock(&self.folders);
        folders.retain(|folder| planned.contains(folder));
        folders.extend(added.iter().cloned());
        added.into_iter().map(|folder| (folder, false)).collect()
    }

    /// Whether changes in `folder` are already reported.
    pub fn covers(&self, folder: &Path) -> bool {
        lock(&self.folders).contains(folder)
    }

    /// Whether changes in `folder`, as it is now, are reported — as far as
    /// [`Self::covers`] knows: `notify` keeps the platform's watches to
    /// itself.
    #[cfg(test)]
    pub fn live(&self, folder: &Path) -> bool {
        self.covers(folder)
    }

    /// Whether a watched folder is `path` or inside it.
    pub fn watches_within(&self, path: &Path) -> bool {
        lock(&self.folders)
            .iter()
            .any(|folder| folder.starts_with(path))
    }
}

/// `event` as inotify would report it: ReadDirectoryChangesW says a path
/// was added or removed, not whether it is a file or a folder, so a path
/// added is looked at, and one removed is a folder if it was watched as one.
/// A folder's own change — its last write, moved by every write in it — is
/// dropped: what is in sight in it is watched itself.
#[cfg(windows)]
fn classified(
    event: notify::Result<notify::Event>,
    folders: &Mutex<BTreeSet<PathBuf>>,
) -> Option<notify::Result<notify::Event>> {
    use notify::event::{CreateKind, RemoveKind};
    let Ok(mut event) = event else {
        return Some(event);
    };
    let path = event.paths.first()?;
    match event.kind {
        EventKind::Create(CreateKind::Any) => {
            event.kind = EventKind::Create(if path.is_dir() {
                CreateKind::Folder
            } else {
                CreateKind::File
            });
        }
        EventKind::Remove(RemoveKind::Any) => {
            event.kind = EventKind::Remove(if lock(folders).contains(path) {
                RemoveKind::Folder
            } else {
                RemoveKind::File
            });
        }
        EventKind::Modify(ModifyKind::Any) if path.is_dir() => return None,
        _ => {}
    }
    Some(Ok(event))
}

/// Forgets the watched folders `event` says are gone: a removed or renamed
/// path, and every folder under it.
fn lost(folders: &Mutex<BTreeSet<PathBuf>>, event: &notify::Event) {
    if !matches!(
        event.kind,
        EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_))
    ) {
        return;
    }
    let mut folders = lock(folders);
    for path in &event.paths {
        folders.retain(|folder| !folder.starts_with(path));
    }
}

fn lock(folders: &Mutex<BTreeSet<PathBuf>>) -> MutexGuard<'_, BTreeSet<PathBuf>> {
    folders
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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
