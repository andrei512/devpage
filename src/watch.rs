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
    let base = document.base.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else { return };
        if event.kind.is_access() {
            return;
        }
        let watched = event.paths.iter().any(|path| {
            let path = path.canonicalize().unwrap_or_else(|_| path.clone());
            path.strip_prefix(&document.base)
                .is_ok_and(|relative| document.watches(relative))
        });
        if watched {
            let _ = changes.send(());
        }
    })?;
    watcher.watch(&base, RecursiveMode::Recursive)?;
    Ok(watcher)
}
