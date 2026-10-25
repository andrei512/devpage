//! Watches the document's folder and announces changes to the files it names.

use std::sync::Arc;

use anyhow::Result;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::broadcast;

use crate::document::Document;

/// Starts watching; every change to a watched file is sent on `changes`. The
/// returned watcher stops when dropped.
pub fn start(
    document: Arc<Document>,
    changes: broadcast::Sender<()>,
) -> Result<RecommendedWatcher> {
    let base = document.repo.clone();
    // What git ignores — build output, dependencies — never refreshes a page.
    let (ignored, _) = ignore::gitignore::Gitignore::new(base.join(".gitignore"));
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else { return };
        if event.kind.is_access() {
            return;
        }
        let inside = event.paths.iter().any(|path| {
            let path = path.canonicalize().unwrap_or_else(|_| path.clone());
            path.strip_prefix(&document.repo).is_ok_and(|relative| {
                !relative.starts_with(".git")
                    && !ignored
                        .matched_path_or_any_parents(relative, path.is_dir())
                        .is_ignore()
            })
        });
        if inside {
            let _ = changes.send(());
        }
    })?;
    watcher.watch(&base, RecursiveMode::Recursive)?;
    Ok(watcher)
}
