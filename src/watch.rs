//! Watches the served folder: keeps the index current, tells open pages a
//! watched file changed, tells them a comment thread changed, and tells the
//! design store every file that changed.

use std::sync::Arc;

use anyhow::Result;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::broadcast;

use crate::designs::Designs;
use crate::document::{Document, Entry};
use crate::index::Index;
use std::path::{Path, PathBuf};

/// Starts watching. A change to a watched file sends `change`; a change to a
/// comment thread sends `comments /<page>`, whatever the site's `watch` says.
/// Every changed file outside `.devpage/` also goes to the design store,
/// whatever `watch` and `.gitignore` say, since any file may be part of a
/// design. The returned watcher stops when dropped.
pub fn start(
    entry: Arc<Entry>,
    index: Arc<Index>,
    designs: Designs,
    changes: broadcast::Sender<String>,
) -> Result<RecommendedWatcher> {
    let served = entry.document().served.clone();
    let root = served.clone();
    let listeners = Listeners {
        served,
        entry,
        index,
        designs,
        changes,
    };
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else { return };
        if event.kind.is_access() {
            return;
        }
        listeners.changed(&event.paths);
    })?;
    watcher.watch(&root, RecursiveMode::Recursive)?;
    Ok(watcher)
}

/// Who hears about files that change: open pages, the index and the design
/// store.
struct Listeners {
    served: PathBuf,
    entry: Arc<Entry>,
    index: Arc<Index>,
    designs: Designs,
    changes: broadcast::Sender<String>,
}

impl Listeners {
    /// `paths` changed, were created or were deleted: tell each listener what
    /// it needs to know.
    fn changed(&self, paths: &[PathBuf]) {
        let Listeners {
            served,
            entry,
            index,
            designs,
            changes,
        } = self;
        let mut refresh = false;
        for path in paths {
            let path = path.canonicalize().unwrap_or_else(|_| path.clone());
            let Ok(relative) = path.strip_prefix(served) else {
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
            if relative.starts_with(".devpage") {
                continue;
            }
            designs.changed(&path);
            index.changed(&path);
            // The entry's own file: re-read it, so `/` and `watch` follow it,
            // and refresh open pages whatever `watch` says.
            if path == entry.file() {
                entry.reload();
                refresh = true;
            } else if !index.ignored(&path, path.is_dir()) && refreshes(entry, index, &path) {
                refresh = true;
            }
        }
        if refresh {
            let _ = changes.send("change".into());
        }
    }
}

/// Whether a change to `path` refreshes open pages. Document files and
/// widget files always do; any other path is judged by the `watch` list of the
/// nearest site holding it, and refreshes when there is no such list.
fn refreshes(entry: &Entry, index: &Index, path: &Path) -> bool {
    if index.kind(path).is_some() || is_widget_file(entry, index, path) {
        return true;
    }
    let Some(site) = index.site_for(path) else {
        return true;
    };
    let document = entry.document();
    if site == document.path() {
        return document.watches(path);
    }
    match Document::load(&site, &document.served) {
        Ok(site) => site.watches(path),
        Err(_) => true,
    }
}

/// Whether `path` is a widget or a helper in the widgets folder of the entry
/// document or of any indexed document.
fn is_widget_file(entry: &Entry, index: &Index, path: &Path) -> bool {
    if !path
        .extension()
        .is_some_and(|ext| ext == "ts" || ext == "tsx")
    {
        return false;
    }
    let document = entry.document();
    let in_folder = |folder: Option<&Path>| {
        folder.is_some_and(|folder| crate::widgets::is_widget_file(path, folder))
    };
    in_folder(document.widgets.as_deref())
        || index.documents().iter().any(|file| {
            Document::load(file, &document.served)
                .is_ok_and(|other| in_folder(other.widgets.as_deref()))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let designs = Designs::new(dir.to_path_buf());
        let _watcher = start(Arc::new(Entry::new(document)), index, designs, changes).unwrap();
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

    /// The hand-off itself, without waiting on the platform's file events: a
    /// changed file reaches the design store, which forgets what it read and
    /// resolves the design again; a file under `.devpage/` never does.
    #[test]
    fn a_changed_file_reaches_the_design_store() {
        let dir = served("designs", "kind: site\ntitle: T\nroot: README.md\n");
        std::fs::create_dir_all(dir.join(".devpage")).unwrap();
        let valid = "<Frame width={10} height={10} />\n";
        let broken = "<Frame width={10} height={10} lable=\"x\" />\n";
        std::fs::write(dir.join("docs/screen.design"), valid).unwrap();
        std::fs::write(dir.join(".devpage/screen.design"), valid).unwrap();
        let document = Arc::new(Document::load(&dir.join("docs"), &dir).unwrap());
        let designs = Designs::new(dir.clone());
        let listeners = Listeners {
            served: dir.clone(),
            entry: Arc::new(Entry::new(document)),
            index: Arc::new(Index::build(&dir)),
            designs: designs.clone(),
            changes: broadcast::channel(64).0,
        };
        assert!(designs.resolve("docs/screen.design").is_ok());
        assert!(designs.resolve(".devpage/screen.design").is_ok());

        // Saved broken: the store answers from memory until it is told.
        std::fs::write(dir.join("docs/screen.design"), broken).unwrap();
        std::fs::write(dir.join(".devpage/screen.design"), broken).unwrap();
        assert!(designs.resolve("docs/screen.design").is_ok());

        listeners.changed(&[
            dir.join("docs/screen.design"),
            dir.join(".devpage/screen.design"),
        ]);
        assert!(designs.resolve("docs/screen.design").is_err());
        assert!(designs.resolve(".devpage/screen.design").is_ok());
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

    #[tokio::test]
    async fn the_watch_list_follows_the_entry_file() {
        let dir = served(
            "relist",
            "kind: site\ntitle: T\nroot: README.md\nwatch: [\"**/*.md\"]\n",
        );
        let document = Arc::new(Document::load(&dir.join("docs"), &dir).unwrap());
        let entry = Arc::new(Entry::new(document));
        let index = Arc::new(Index::build(&dir));
        let (changes, mut received) = broadcast::channel(64);
        let designs = Designs::new(dir.clone());
        let _watcher = start(entry.clone(), index, designs, changes).unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Its own file changing refreshes pages, though `watch` names only Markdown.
        std::fs::write(
            dir.join("docs/devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\nwatch: [\"**/*.svg\"]\n",
        )
        .unwrap();
        let drain = |received: &mut broadcast::Receiver<String>| {
            let mut messages = Vec::new();
            while let Ok(message) = received.try_recv() {
                messages.push(message);
            }
            messages
        };
        tokio::time::sleep(Duration::from_millis(800)).await;
        let messages = drain(&mut received);
        assert!(messages.iter().any(|m| m == "change"), "{messages:?}");

        // Now Markdown is outside `watch`, and an SVG is in it.
        std::fs::write(dir.join("docs/README.md"), "# Changed").unwrap();
        tokio::time::sleep(Duration::from_millis(800)).await;
        let messages = drain(&mut received);
        assert!(!messages.iter().any(|m| m == "change"), "{messages:?}");
        std::fs::write(dir.join("docs/logo.svg"), "<svg/>").unwrap();
        tokio::time::sleep(Duration::from_millis(800)).await;
        let messages = drain(&mut received);
        assert!(messages.iter().any(|m| m == "change"), "{messages:?}");
    }

    #[tokio::test]
    async fn a_sites_watch_list_narrows_only_its_own_folder() {
        // The entry site has no `watch`; a second site watches only text files.
        let dir = served("other", "kind: site\ntitle: T\nroot: README.md\n");
        std::fs::create_dir_all(dir.join("other")).unwrap();
        std::fs::write(dir.join("other/index.md"), "# Other").unwrap();
        std::fs::write(
            dir.join("other/devpage.yml"),
            "kind: site\ntitle: O\nroot: index.md\nwatch: [\"*.txt\"]\n",
        )
        .unwrap();
        let quiet = after(&dir, || {
            std::fs::write(dir.join("other/index.md"), "# Changed").unwrap()
        })
        .await;
        assert!(!quiet.iter().any(|m| m == "change"), "{quiet:?}");
        let loud = after(&dir, || {
            std::fs::write(dir.join("other/notes.txt"), "x").unwrap()
        })
        .await;
        assert!(loud.iter().any(|m| m == "change"), "{loud:?}");
        // The entry's own pages still refresh, and so does a doc in no site.
        let entry = after(&dir, || {
            std::fs::write(dir.join("docs/README.md"), "# Changed").unwrap()
        })
        .await;
        assert!(entry.iter().any(|m| m == "change"), "{entry:?}");
        std::fs::create_dir_all(dir.join("plan")).unwrap();
        let loose = after(&dir, || {
            std::fs::write(dir.join("plan/design.md"), "# Plan").unwrap()
        })
        .await;
        assert!(loose.iter().any(|m| m == "change"), "{loose:?}");
    }

    #[tokio::test]
    async fn the_entrys_watch_list_does_not_quiet_another_folder() {
        let dir = served(
            "entry-watch",
            "kind: site\ntitle: T\nroot: README.md\nwatch: [\"*.svg\"]\n",
        );
        std::fs::create_dir_all(dir.join("examples")).unwrap();
        let messages = after(&dir, || {
            std::fs::write(dir.join("examples/design.md"), "# Design").unwrap()
        })
        .await;
        assert!(messages.iter().any(|m| m == "change"), "{messages:?}");
    }

    #[tokio::test]
    async fn widget_files_refresh_whatever_watch_says() {
        let dir = served(
            "widgets",
            "kind: site\ntitle: T\nroot: README.md\nwidgets: widgets\nwatch: [\"**/*.md\"]\n",
        );
        std::fs::create_dir_all(dir.join("docs/widgets")).unwrap();
        std::fs::create_dir_all(dir.join("docs/other")).unwrap();
        let quiet = after(&dir, || {
            std::fs::write(dir.join("docs/other/app.tsx"), "x").unwrap(); // not a widgets folder
        })
        .await;
        assert!(!quiet.iter().any(|m| m == "change"), "{quiet:?}");
        let loud = after(&dir, || {
            std::fs::write(dir.join("docs/widgets/format.ts"), "export const f = 1;").unwrap()
        })
        .await;
        assert!(loud.iter().any(|m| m == "change"), "{loud:?}");
    }
}
