//! Watches the served folder: keeps the index current, tells open pages a
//! watched file changed, and tells them a comment thread changed.

use std::sync::Arc;

use anyhow::Result;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::broadcast;

use crate::document::Document;
use crate::index::Index;

/// Starts watching. A change to a watched file sends `change`; a change to a
/// comment thread sends `comments /<page>`, whatever the site's `watch` says.
/// The returned watcher stops when dropped.
pub fn start(
    document: Arc<Document>,
    index: Arc<Index>,
    changes: broadcast::Sender<String>,
) -> Result<RecommendedWatcher> {
    let served = document.served.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else { return };
        if event.kind.is_access() {
            return;
        }
        let mut refresh = false;
        for path in &event.paths {
            let path = path.canonicalize().unwrap_or_else(|_| path.clone());
            let Ok(relative) = path.strip_prefix(&document.served) else {
                continue;
            };
            // A thread changed: tell the pages showing its page to fetch comments.
            if let Ok(thread) = relative.strip_prefix(".devpage/comments") {
                if thread.extension().is_some_and(|ext| ext == "yml")
                    && let Some(page) = thread.parent()
                {
                    let _ = changes.send(format!("comments /{}", page.display()));
                }
                continue;
            }
            index.changed(&path);
            if !index.ignored(&path, path.is_dir()) && document.watches(&path) {
                refresh = true;
            }
        }
        if refresh {
            let _ = changes.send("change".into());
        }
    })?;
    watcher.watch(&served, RecursiveMode::Recursive)?;
    Ok(watcher)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    fn served(name: &str, site: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("devpage-watch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("docs/build")).unwrap();
        std::fs::write(dir.join("docs/README.md"), "# Home").unwrap();
        std::fs::write(dir.join("docs/devpage.yml"), site).unwrap();
        std::fs::write(dir.join("docs/.gitignore"), "build/\n").unwrap();
        dir.canonicalize().unwrap()
    }

    /// The messages that arrive within a short while after `touch` runs.
    async fn after(dir: &Path, touch: impl FnOnce()) -> Vec<String> {
        let document = Arc::new(Document::load(&dir.join("docs"), dir).unwrap());
        let index = Arc::new(Index::build(dir));
        let (changes, mut received) = broadcast::channel(64);
        let _watcher = start(document, index, changes).unwrap();
        // Let the platform's watcher settle before changing anything.
        tokio::time::sleep(Duration::from_millis(300)).await;
        touch();
        let mut messages = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_millis(1500);
        while let Ok(Ok(message)) = tokio::time::timeout_at(deadline, received.recv()).await {
            messages.push(message);
        }
        messages
    }

    #[tokio::test]
    async fn a_saved_page_refreshes_open_pages() {
        let dir = served("page", "kind: site\ntitle: T\nroot: README.md\n");
        let messages = after(&dir, || {
            std::fs::write(dir.join("docs/README.md"), "# Changed").unwrap()
        })
        .await;
        assert!(messages.iter().any(|m| m == "change"), "{messages:?}");
    }

    #[tokio::test]
    async fn ignored_files_and_files_outside_watch_do_not_refresh() {
        let dir = served(
            "quiet",
            "kind: site\ntitle: T\nroot: README.md\nwatch: [\"**/*.md\"]\n",
        );
        let messages = after(&dir, || {
            std::fs::write(dir.join("docs/build/out.md"), "x").unwrap(); // nested .gitignore
            std::fs::write(dir.join("docs/logo.svg"), "<svg/>").unwrap(); // outside watch
        })
        .await;
        assert!(!messages.iter().any(|m| m == "change"), "{messages:?}");
    }

    #[tokio::test]
    async fn a_comment_thread_always_reaches_its_page() {
        let dir = served(
            "comments",
            "kind: site\ntitle: T\nroot: README.md\nwatch: [\"**/*.md\"]\n",
        );
        let messages = after(&dir, || {
            let thread = dir.join(".devpage/comments/docs/README.md");
            std::fs::create_dir_all(&thread).unwrap();
            std::fs::write(thread.join("20261025T000000Z-abcd.yml"), "thread: []\n").unwrap();
        })
        .await;
        assert!(
            messages.iter().any(|m| m == "comments /docs/README.md"),
            "{messages:?}"
        );
        assert!(!messages.iter().any(|m| m == "change"), "{messages:?}");
    }
}
