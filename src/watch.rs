//! Watches the served folder: keeps the index current, tells open pages a
//! watched file changed, tells them a comment thread changed, and tells the
//! design store every file that changed and isn't out of sight.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Result;
use notify::event::{CreateKind, EventKind, RemoveKind};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::broadcast;

use crate::designs::Designs;
use crate::document::{Document, Entry};
use crate::index::Index;
use std::path::{Path, PathBuf};

/// How long a burst of file events is collected before it is handled: each
/// path once, and at most one `change` for the whole burst.
const BURST: Duration = Duration::from_millis(50);

/// Starts watching. A change to a watched file sends `change`; a change to a
/// comment thread sends `comments /<page>`, whatever the site's `watch` says.
/// A path out of sight — under `.git`, or excluded by a `.gitignore` and not
/// named by its site's `watch` — is dropped as it arrives, before any
/// listener hears of it. Every other changed file outside `.devpage/` also
/// goes to the design store, whatever `watch` says, since any file may be
/// part of a design. The returned watcher stops when dropped, and so does
/// the thread handling its events.
pub fn start(
    entry: Arc<Entry>,
    index: Arc<Index>,
    designs: Designs,
    changes: broadcast::Sender<String>,
) -> Result<RecommendedWatcher> {
    let root = entry.document().served.clone();
    let listeners = Listeners {
        served: root.clone(),
        entry,
        index,
        designs,
        changes,
    };
    let mut watcher = notify::recommended_watcher(intake(listeners))?;
    watcher.watch(&root, RecursiveMode::Recursive)?;
    Ok(watcher)
}

/// The handler for the platform's watcher: it sifts each event's paths as
/// they arrive and passes the survivors to a thread that handles them in
/// bursts. The thread ends when the handler is dropped.
fn intake(listeners: Listeners) -> impl FnMut(notify::Result<notify::Event>) + Send + 'static {
    let listeners = Arc::new(listeners);
    let (paths, arriving) = mpsc::channel::<PathBuf>();
    let handler = {
        let listeners = listeners.clone();
        move |event: notify::Result<notify::Event>| {
            for path in listeners.sift(event) {
                let _ = paths.send(path);
            }
        }
    };
    std::thread::spawn(move || {
        while let Some(burst) = next_burst(&arriving) {
            listeners.changed(&burst);
        }
    });
    handler
}

/// Waits for a path, then collects whatever else arrives within [`BURST`],
/// each path once, in the order it first arrived. `None` once every sender
/// is gone and nothing is left.
fn next_burst(arriving: &mpsc::Receiver<PathBuf>) -> Option<Vec<PathBuf>> {
    let first = arriving.recv().ok()?;
    let deadline = Instant::now() + BURST;
    let mut seen = HashSet::from([first.clone()]);
    let mut burst = vec![first];
    while let Ok(path) = arriving.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        if seen.insert(path.clone()) {
            burst.push(path);
        }
    }
    Some(burst)
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
    /// The paths of `event` that can matter to a listener, decided from the
    /// path alone — no file system call — so a build rewriting an ignored
    /// folder costs a lookup per path and nothing more. Accesses and errors
    /// carry none.
    fn sift(&self, event: notify::Result<notify::Event>) -> Vec<PathBuf> {
        let Ok(event) = event else {
            return Vec::new();
        };
        if event.kind.is_access() {
            return Vec::new();
        }
        let is_dir = matches!(
            event.kind,
            EventKind::Create(CreateKind::Folder) | EventKind::Remove(RemoveKind::Folder)
        );
        event
            .paths
            .into_iter()
            .filter(|path| self.can_matter(path, is_dir))
            .collect()
    }

    /// Whether a change to `path` can matter: not under `.git`, and either in
    /// sight or named by its site's `watch` list. A comment thread always
    /// matters; nothing else under `.devpage/` does. A path not under the
    /// served folder as reported is kept, to be judged once canonical.
    fn can_matter(&self, path: &Path, is_dir: bool) -> bool {
        let Ok(relative) = path.strip_prefix(&self.served) else {
            return true;
        };
        if relative.starts_with(".devpage/comments") {
            return true;
        }
        if relative.starts_with(".devpage")
            || relative.components().any(|part| part.as_os_str() == ".git")
        {
            return false;
        }
        !self.index.ignored(path, is_dir) || self.named_by_watch(path)
    }

    /// Whether the `watch` list of the nearest site holding `path` names it.
    fn named_by_watch(&self, path: &Path) -> bool {
        let Some(site) = self.index.site_for(path) else {
            return false;
        };
        let document = self.entry.document();
        if site == document.path() {
            return document.names(path);
        }
        Document::load(&site, &document.served).is_ok_and(|site| site.names(path))
    }

    /// `paths` changed, were created or were deleted: tell each listener what
    /// it needs to know, and send at most one `change`.
    fn changed(&self, paths: &[PathBuf]) {
        let Listeners {
            served,
            entry,
            index,
            designs,
            changes,
        } = self;
        let mut refresh = false;
        let mut searched = Vec::new();
        for path in paths {
            let path = canonical(path);
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
            crate::widgets::forget(&path);
            index.changed(&path);
            searched.push(path.clone());
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
        // After the pages hear, so indexing text never holds up a refresh.
        if let Some(search) = index.search() {
            search.changed(index, &searched);
        }
    }
}

/// `path` canonicalized, or as reported when it can't be (a deleted file).
/// Only paths that survived sifting get here.
fn canonical(path: &Path) -> PathBuf {
    #[cfg(test)]
    tests::CANONICALIZED.with(|count| count.set(count.get() + 1));
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
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
    use notify::event::{DataChange, ModifyKind};

    thread_local! {
        /// How many paths this thread has canonicalized.
        pub static CANONICALIZED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    /// The listeners of the served folder `dir`, started on `docs/`.
    fn listeners(dir: &Path, changes: broadcast::Sender<String>) -> Listeners {
        let document = Arc::new(Document::load(&dir.join("docs"), dir).unwrap());
        Listeners {
            served: dir.to_path_buf(),
            entry: Arc::new(Entry::new(document)),
            index: Arc::new(Index::build(dir)),
            designs: Designs::new(dir.to_path_buf()),
            changes,
        }
    }

    /// The event a platform watcher reports when `paths` were written.
    fn written(paths: &[PathBuf]) -> notify::Result<notify::Event> {
        let mut event = notify::Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Any)));
        event.paths = paths.to_vec();
        Ok(event)
    }

    /// Every message already sent.
    fn drain(received: &mut broadcast::Receiver<String>) -> Vec<String> {
        let mut messages = Vec::new();
        while let Ok(message) = received.try_recv() {
            messages.push(message);
        }
        messages
    }

    #[test]
    fn build_output_git_and_ignored_folders_never_reach_a_listener() {
        let dir = served("sift", "kind: site\ntitle: T\nroot: README.md\n");
        std::fs::write(dir.join(".gitignore"), "/target/\n").unwrap();
        std::fs::create_dir_all(dir.join("target/debug")).unwrap();
        std::fs::create_dir_all(dir.join(".git/objects")).unwrap();
        // A design made from a file in an ignored folder: told, it would resolve again.
        std::fs::write(
            dir.join("docs/build/screen.design"),
            "<Frame width={10} height={10} />\n",
        )
        .unwrap();
        let (changes, mut received) = broadcast::channel(64);
        let listeners = listeners(&dir, changes);
        assert!(
            listeners
                .designs
                .resolve("docs/build/screen.design")
                .is_ok()
        );
        std::fs::write(
            dir.join("docs/build/screen.design"),
            "<Frame width={10} height={10} lable=\"x\" />\n",
        )
        .unwrap();

        let noise = [
            dir.join("target/debug/devpage"),
            dir.join("target/debug"),
            dir.join(".git/objects/ab"),
            dir.join(".git/index"),
            dir.join("docs/build/out.md"),
            dir.join("docs/build/screen.design"),
            dir.join(".devpage/screen.design"),
        ];
        assert_eq!(listeners.sift(written(&noise)), Vec::<PathBuf>::new());
        let mut folder = notify::Event::new(EventKind::Create(CreateKind::Folder));
        folder.paths = vec![dir.join("docs/build")];
        assert_eq!(listeners.sift(Ok(folder)), Vec::<PathBuf>::new());

        let before = CANONICALIZED.with(|count| count.get());
        listeners.changed(&listeners.sift(written(&noise)));
        assert_eq!(CANONICALIZED.with(|count| count.get()), before);
        assert_eq!(drain(&mut received), Vec::<String>::new());
        assert!(
            listeners
                .designs
                .resolve("docs/build/screen.design")
                .is_ok()
        );
    }

    #[test]
    fn comment_threads_and_files_watch_names_still_arrive() {
        let dir = served(
            "named",
            "kind: site\ntitle: T\nroot: README.md\nwatch: [\"**/*.md\", \"build/*.svg\"]\n",
        );
        let (changes, mut received) = broadcast::channel(64);
        let listeners = listeners(&dir, changes);
        let thread = dir.join(".devpage/comments/docs/README.md/20261025T000000Z-abcd.yml");
        let named = dir.join("docs/build/chart.svg");
        let unnamed = dir.join("docs/build/chart.png");
        let page = dir.join("docs/README.md");
        let arrived = listeners.sift(written(&[
            thread.clone(),
            named.clone(),
            unnamed,
            page.clone(),
        ]));
        assert_eq!(arrived, vec![thread, named, page]);
        listeners.changed(&arrived);
        assert_eq!(drain(&mut received), ["comments /docs/README.md", "change"]);
    }

    #[test]
    fn a_new_gitignore_rule_takes_effect_without_a_restart() {
        let dir = served("rule", "kind: site\ntitle: T\nroot: README.md\n");
        std::fs::create_dir_all(dir.join("docs/drafts")).unwrap();
        let listeners = listeners(&dir, broadcast::channel(64).0);
        let draft = dir.join("docs/drafts/idea.md");
        assert_eq!(
            listeners.sift(written(std::slice::from_ref(&draft))),
            vec![draft.clone()]
        );

        std::fs::write(dir.join("docs/.gitignore"), "build/\ndrafts/\n").unwrap();
        let rule = dir.join("docs/.gitignore");
        listeners.changed(&listeners.sift(written(&[rule])));
        assert_eq!(listeners.sift(written(&[draft])), Vec::<PathBuf>::new());
    }

    #[test]
    fn the_search_index_hears_of_saved_and_deleted_pages() {
        let dir = served("search", "kind: site\ntitle: T\nroot: README.md\n");
        std::fs::write(
            dir.join("docs/README.md"),
            "# Home\n\n## Topics\n\n- [Guide](guide.md)\n",
        )
        .unwrap();
        let page = dir.join("docs/guide.md");
        std::fs::write(&page, "# Guide\n\nAn otter swims.\n").unwrap();
        let mut listeners = listeners(&dir, broadcast::channel(64).0);
        listeners.index = Arc::new(Index::build(&dir).with_search(&dir.join("docs/devpage.yml")));
        let found = |word: &str| {
            let search = listeners.index.search().unwrap();
            search
                .search(word)
                .pages
                .into_iter()
                .flat_map(|page| page.sections)
                .map(|section| section.url)
                .collect::<Vec<_>>()
        };
        assert_eq!(found("otter"), ["/docs/guide.md#guide"]);

        std::fs::write(&page, "# Guide\n\nA beaver builds.\n").unwrap();
        listeners.changed(&listeners.sift(written(std::slice::from_ref(&page))));
        assert_eq!(found("beaver"), ["/docs/guide.md#guide"]);
        assert!(found("otter").is_empty());

        std::fs::remove_file(&page).unwrap();
        let mut removed = notify::Event::new(EventKind::Remove(RemoveKind::File));
        removed.paths = vec![page];
        listeners.changed(&listeners.sift(Ok(removed)));
        assert!(found("beaver").is_empty());
    }

    #[test]
    fn a_burst_of_changes_to_one_file_sends_one_change() {
        let dir = served("burst", "kind: site\ntitle: T\nroot: README.md\n");
        let (changes, mut received) = broadcast::channel(64);
        let mut handler = intake(listeners(&dir, changes));
        // Saves trickling in, as an editor or a build writes, within one burst.
        for _ in 0..5 {
            handler(written(&[dir.join("docs/README.md")]));
            std::thread::sleep(BURST / 25);
        }
        std::thread::sleep(BURST * 6);
        assert_eq!(drain(&mut received), ["change"]);

        // A later burst is a new one.
        handler(written(&[dir.join("docs/README.md")]));
        std::thread::sleep(BURST * 6);
        assert_eq!(drain(&mut received), ["change"]);
    }

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
