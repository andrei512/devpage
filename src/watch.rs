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
    changes: broadcast::Sender<String>,
) -> Result<RecommendedWatcher> {
    let base = document.repo.clone();
    // What git ignores — build output, dependencies — never refreshes a page.
    let (ignored, _) = ignore::gitignore::Gitignore::new(base.join(".gitignore"));
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else { return };
        if event.kind.is_access() {
            return;
        }
        for path in &event.paths {
            let path = path.canonicalize().unwrap_or_else(|_| path.clone());
            let Ok(relative) = path.strip_prefix(&document.repo) else {
                continue;
            };
            // A thread changed: tell the pages showing its page to fetch comments.
            if let Ok(thread) = relative.strip_prefix(".devpage/comments") {
                if thread.extension().is_some_and(|ext| ext == "yml")
                    && let Some(page) = thread.parent()
                {
                    let _ = changes.send(format!("comments /{}", page.display()));
                }
                return;
            }
            let skipped = relative.starts_with(".git")
                || relative.starts_with(".devpage")
                || ignored
                    .matched_path_or_any_parents(relative, path.is_dir())
                    .is_ignore();
            if !skipped {
                let _ = changes.send("change".into());
                return;
            }
        }
    })?;
    watcher.watch(&base, RecursiveMode::Recursive)?;
    Ok(watcher)
}
