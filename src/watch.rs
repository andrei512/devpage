//! Watches the served folder: keeps the index current, tells open pages a
//! watched file changed, tells them a comment thread changed, and tells the
//! design store every file that changed and isn't out of sight. Only what is
//! in sight is watched (the watch set, `set.rs`), through the platform's
//! watcher: `macos.rs`, `inotify.rs`, or `recursive.rs` elsewhere.

mod set;

#[cfg(target_os = "linux")]
mod inotify;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod recursive;

#[cfg(target_os = "linux")]
use inotify::Platform;
#[cfg(target_os = "macos")]
use macos::Platform;
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
use recursive::Platform;

use std::collections::HashSet;
use std::hash::Hash;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use anyhow::Result;
use notify::event::{CreateKind, EventKind, RemoveKind};

use crate::changes::{Change, Changes, Scope};
use crate::comments::yaml::Yaml;
use crate::designs::Designs;
use crate::document::{Document, Entry, KindName};
use crate::index::Index;
use crate::served::ServedFolder;
use set::WatchSet;
use std::path::{Path, PathBuf};

/// How long a burst of file events is collected before it is handled: each
/// path once, and at most one `change` for the whole burst.
const BURST: Duration = Duration::from_millis(50);

/// What the platform's watcher calls with each event, from any thread.
type Handler = Arc<dyn Fn(notify::Result<notify::Event>) + Send + Sync>;

/// A running watch. Dropping it stops the platform's watcher, and with it
/// the thread handling its events.
pub struct Watch {
    _platform: Arc<Mutex<Platform>>,
}

/// Starts watching. A change to a watched file publishes a refresh on the
/// change bus; a change to a comment thread publishes that page's comments,
/// whatever the site's `watch` says — both for the documents at `/` — unless
/// `threads`, the comment store the server writes through, made it itself:
/// that write was announced as it landed.
/// A path out of sight — under `.git`, or excluded by a `.gitignore` and not
/// named by its site's `watch` — is dropped as it arrives, before any
/// listener hears of it. Every other changed file outside `.devpage/` also
/// goes to the design store, whatever `watch` says, since any file may be
/// part of a design.
pub fn start(
    entry: Arc<Entry>,
    index: Arc<Index>,
    designs: Designs,
    changes: Changes,
    threads: Arc<Yaml>,
) -> Result<Watch> {
    let root = entry.document().served.clone();
    let set = WatchSet::plan(root.path(), &index, &entry);
    let listeners = Listeners {
        served: root.clone(),
        entry,
        index,
        designs,
        changes,
        scope: Scope::root(),
        threads,
    };
    let watching = Arc::new(OnceLock::new());
    let mut platform = Platform::new(root.path(), intake(listeners, watching.clone()))?;
    platform.apply(set);
    let platform = Arc::new(Mutex::new(platform));
    let _ = watching.set(Arc::downgrade(&platform));
    Ok(Watch {
        _platform: platform,
    })
}

/// The platform's watcher, once started, for the burst thread to re-plan.
type Watching = Arc<OnceLock<Weak<Mutex<Platform>>>>;

/// What the handler passes the burst thread.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Arrival {
    /// A path that survived sifting.
    Changed(PathBuf),
    /// A folder created out of sight directly in a folder in sight: the
    /// watch set may have to leave it out.
    Hidden(PathBuf),
}

/// The handler for the platform's watcher: it sifts each event's paths as
/// they arrive and passes the survivors to a thread that handles them in
/// bursts, re-planning the watch set when a burst changes it. The thread
/// ends when the handler is dropped.
fn intake(listeners: Listeners, watching: Watching) -> Handler {
    let listeners = Arc::new(listeners);
    let (arrivals, arriving) = mpsc::channel::<Arrival>();
    let handler: Handler = {
        let listeners = listeners.clone();
        Arc::new(move |event: notify::Result<notify::Event>| {
            #[cfg(test)]
            tests::record(&event);
            if let Ok(event) = &event {
                for folder in listeners.hidden(event) {
                    let _ = arrivals.send(Arrival::Hidden(folder));
                }
            }
            for path in listeners.sift(event) {
                let _ = arrivals.send(Arrival::Changed(path));
            }
        })
    };
    std::thread::spawn(move || {
        while let Some(burst) = next_burst(&arriving) {
            let mut hidden = false;
            let paths: Vec<PathBuf> = burst
                .into_iter()
                .filter_map(|arrival| match arrival {
                    Arrival::Changed(path) => Some(path),
                    Arrival::Hidden(_) => {
                        hidden = true;
                        None
                    }
                })
                .collect();
            listeners.changed(&paths);
            if let Some(platform) = watching.get().and_then(Weak::upgrade) {
                let mut platform = platform.lock().unwrap_or_else(|p| p.into_inner());
                if hidden || listeners.reshapes(&platform, &paths) {
                    listeners.replan(&mut platform);
                }
            }
        }
    });
    handler
}

/// Waits for an arrival, then collects whatever else arrives within
/// [`BURST`], each once, in the order it first arrived. `None` once every
/// sender is gone and nothing is left.
fn next_burst<T: Clone + Eq + Hash>(arriving: &mpsc::Receiver<T>) -> Option<Vec<T>> {
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

/// Who hears about files that change: the caches that forget what changed —
/// the index (with the widget modules it owns), the design store, the entry
/// document and the search index — and, through the change bus, open pages.
struct Listeners {
    served: ServedFolder,
    entry: Arc<Entry>,
    index: Arc<Index>,
    designs: Designs,
    changes: Changes,
    /// The documents the watched folder's changes concern.
    scope: Scope,
    /// The comment store the server writes through, which knows the thread
    /// files it wrote — and announced — itself.
    threads: Arc<Yaml>,
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
        let Some(relative) = self.served.relative(path) else {
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

    /// The folders `event` says were created out of sight directly in a
    /// folder in sight — a build's first `target/` — judged from the path
    /// alone, as sifting is.
    fn hidden(&self, event: &notify::Event) -> Vec<PathBuf> {
        if event.kind != EventKind::Create(CreateKind::Folder) {
            return Vec::new();
        }
        event
            .paths
            .iter()
            .filter(|path| {
                !self.can_matter(path, true)
                    && path.parent().is_some_and(|parent| {
                        self.served.contains(parent) && self.can_matter(parent, true)
                    })
            })
            .cloned()
            .collect()
    }

    /// Whether the burst of `paths` can change the watch set: a `.gitignore`
    /// or a site's document file changed, a folder the platform doesn't
    /// report on appeared, or one holding a watched folder went away.
    fn reshapes(&self, platform: &Platform, paths: &[PathBuf]) -> bool {
        paths.iter().any(|path| {
            let document_file = path
                .extension()
                .is_some_and(|ext| ext == "yml" || ext == "yaml");
            path.file_name().is_some_and(|name| name == ".gitignore")
                || path == self.entry.file()
                || self.index.kind(path) == Some(KindName::Site)
                || (document_file && !path.exists())
                || if path.is_dir() {
                    !platform.covers(path)
                } else {
                    platform.watches_within(path)
                }
        })
    }

    /// Plans the watch set again and has the platform watch it. What is
    /// already in a folder the platform didn't report on before is handled
    /// as changed — a thread written into a new comment folder before its
    /// watch started still reaches its page.
    fn replan(&self, platform: &mut Platform) {
        let mut found = Vec::new();
        for (folder, deep) in
            platform.apply(WatchSet::plan(self.served.path(), &self.index, &self.entry))
        {
            within(&folder, deep, &mut found);
            found.push(folder);
        }
        found.retain(|path| self.can_matter(path, path.is_dir()));
        self.changed(&found);
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

    /// `paths` changed, were created or were deleted: every cache forgets
    /// what it read of them, open pages hear what they should show — at most
    /// one refresh — and then the search index reads them again.
    fn changed(&self, paths: &[PathBuf]) {
        let (heard, searched) = self.forget(paths);
        for change in heard {
            self.changes.publish(&self.scope, change);
        }
        // After the pages hear, so indexing text never holds up a refresh.
        if let Some(search) = self.index.search() {
            search.changed(&self.index, &searched);
        }
    }

    /// The caches forget what they read of `paths`; the changes open pages
    /// should hear come back — a page's comments as each thread changes, then
    /// at most one refresh — with the paths the search index should read
    /// again. Nothing is published.
    fn forget(&self, paths: &[PathBuf]) -> (Vec<Change>, Vec<PathBuf>) {
        let Listeners {
            served,
            entry,
            index,
            designs,
            threads,
            ..
        } = self;
        let mut heard = Vec::new();
        let mut refresh = false;
        let mut searched = Vec::new();
        for path in paths {
            let path = canonical(path);
            let Some(relative) = served.relative(&path) else {
                continue;
            };
            // A thread changed: tell the pages showing its page to fetch
            // comments — unless the store's own write was announced already.
            if let Ok(thread) = relative.strip_prefix(".devpage/comments") {
                if thread.extension().is_some_and(|ext| ext == "yml")
                    && let Some(page) = thread.parent()
                    && threads.unheard(&path)
                {
                    heard.push(Change::Comments(format!("/{}", page.display())));
                }
                continue;
            }
            if relative.starts_with(".devpage") {
                continue;
            }
            designs.changed(&path);
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
            heard.push(Change::Refresh);
        }
        (heard, searched)
    }
}

/// The files and folders directly in `folder`, and when `deep` everything
/// under it, not following symbolic links.
fn within(folder: &Path, deep: bool, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        let path = entry.path();
        if deep && entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            within(&path, deep, found);
        }
        found.push(path);
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

    /// Every path a handler was handed, by any watcher in any test, before
    /// sifting: what the platform reported.
    static RAW: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

    pub fn record(event: &notify::Result<notify::Event>) {
        if let Ok(event) = event {
            RAW.lock().unwrap().extend(event.paths.iter().cloned());
        }
    }

    /// How many paths the platform has reported in `folder`, or that are it.
    fn raw(folder: &Path) -> usize {
        RAW.lock()
            .unwrap()
            .iter()
            .filter(|path| path.starts_with(folder))
            .count()
    }

    /// Starts watching the served folder `dir`, started on `docs/`, and
    /// lets the platform's watcher settle.
    async fn watching(dir: &Path) -> Watch {
        let document = Arc::new(Document::load(&dir.join("docs"), dir).unwrap());
        let watch = start(
            Arc::new(Entry::new(document)),
            Arc::new(Index::build(dir)),
            Designs::new(dir.to_path_buf()),
            Changes::default(),
            Arc::new(Yaml::new(dir)),
        )
        .unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        watch
    }

    /// Waits until the platform has reported something in `folder` since
    /// `before`, and says whether it did within two seconds.
    async fn reported(folder: &Path, before: usize) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if raw(folder) > before {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    }

    /// Writes a few files in `folder`, making it first.
    fn write_in(folder: &Path) {
        std::fs::create_dir_all(folder).unwrap();
        for n in 0..20 {
            std::fs::write(folder.join(format!("f{n}")), "x").unwrap();
        }
    }

    #[tokio::test]
    async fn writes_out_of_sight_are_never_reported() {
        let dir = served("unseen", "kind: site\ntitle: T\nroot: README.md\n");
        std::fs::write(dir.join(".gitignore"), "/target/\n").unwrap();
        std::fs::create_dir_all(dir.join("target/debug")).unwrap();
        std::fs::create_dir_all(dir.join(".git/objects")).unwrap();
        let _watch = watching(&dir).await;
        write_in(&dir.join("target/debug"));
        write_in(&dir.join("target/debug/deps"));
        write_in(&dir.join(".git/objects/ab"));
        // Something in sight, so the quiet below is the platform's, not a stall.
        let before = raw(&dir.join("docs/README.md"));
        std::fs::write(dir.join("docs/README.md"), "# Changed").unwrap();
        assert!(reported(&dir.join("docs/README.md"), before).await);
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(raw(&dir.join("target")), 0);
        assert_eq!(raw(&dir.join(".git")), 0);
    }

    #[tokio::test]
    async fn writes_in_sight_are_reported_shallow_and_deep() {
        let dir = served("seen", "kind: site\ntitle: T\nroot: README.md\n");
        std::fs::write(dir.join(".gitignore"), "/target/\n").unwrap();
        std::fs::create_dir_all(dir.join("target")).unwrap();
        std::fs::write(dir.join("notes.md"), "# Notes").unwrap();
        let _watch = watching(&dir).await;

        // A file directly in the served folder, which is shallow.
        let notes = dir.join("notes.md");
        std::fs::write(&notes, "# Changed").unwrap();
        assert!(reported(&notes, 0).await, "a file in a shallow folder");
        // A file in a deep folder.
        std::fs::write(dir.join("docs/README.md"), "# Changed").unwrap();
        assert!(
            reported(&dir.join("docs/README.md"), 0).await,
            "a deep folder"
        );
        // A new file directly in the served folder.
        let new = dir.join("new.md");
        std::fs::write(&new, "# New").unwrap();
        assert!(reported(&new, 0).await, "a new file in a shallow folder");
        // A new folder in sight, then a file in it.
        std::fs::create_dir(dir.join("plan")).unwrap();
        assert!(reported(&dir.join("plan"), 0).await, "a new folder");
        tokio::time::sleep(Duration::from_millis(300)).await;
        let page = dir.join("plan/design.md");
        std::fs::write(&page, "# Plan").unwrap();
        assert!(reported(&page, 0).await, "a file in a new folder");
    }

    #[tokio::test]
    async fn a_new_gitignore_rule_stops_a_folders_events() {
        let dir = served("gen", "kind: site\ntitle: T\nroot: README.md\n");
        std::fs::write(dir.join(".gitignore"), "/target/\n").unwrap();
        std::fs::create_dir(dir.join("gen")).unwrap();
        let _watch = watching(&dir).await;
        write_in(&dir.join("gen"));
        assert!(reported(&dir.join("gen/f19"), 0).await);

        std::fs::write(dir.join(".gitignore"), "/target/\n/gen/\n").unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        let before = raw(&dir.join("gen"));
        write_in(&dir.join("gen"));
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(raw(&dir.join("gen")), before);
    }

    #[tokio::test]
    async fn a_nested_build_folder_stops_its_events_once_made() {
        let dir = served("crate", "kind: site\ntitle: T\nroot: README.md\n");
        std::fs::create_dir_all(dir.join("crate/src")).unwrap();
        std::fs::write(dir.join("crate/.gitignore"), "target/\n").unwrap();
        let _watch = watching(&dir).await;
        // `cargo build` in the crate: its first writes may still be reported.
        write_in(&dir.join("crate/target/debug"));
        tokio::time::sleep(Duration::from_millis(500)).await;
        let before = raw(&dir.join("crate/target"));
        write_in(&dir.join("crate/target/debug"));
        write_in(&dir.join("crate/target/release"));
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(raw(&dir.join("crate/target")), before);
        // The rest of the crate is still watched.
        std::fs::write(dir.join("crate/src/lib.rs"), "").unwrap();
        assert!(reported(&dir.join("crate/src/lib.rs"), 0).await);
    }

    #[tokio::test]
    async fn a_folder_deleted_and_made_again_in_one_burst_is_watched_again() {
        let dir = served("again", "kind: site\ntitle: T\nroot: README.md\n");
        std::fs::write(dir.join(".gitignore"), "/crate/target/\n").unwrap();
        // `crate/` holds something out of sight, so it is watched shallow.
        std::fs::create_dir_all(dir.join("crate/target")).unwrap();
        std::fs::write(dir.join("crate/a.md"), "# A").unwrap();
        let _watch = watching(&dir).await;
        std::fs::remove_dir_all(dir.join("crate")).unwrap();
        std::fs::create_dir_all(dir.join("crate/target")).unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        for n in 0..3 {
            let later = dir.join(format!("crate/later{n}.md"));
            std::fs::write(&later, "# Later").unwrap();
            assert!(reported(&later, 0).await, "crate/later{n}.md");
        }
    }

    #[tokio::test]
    async fn a_deep_folder_deleted_and_made_again_is_watched_again() {
        let dir = served("checkout", "kind: site\ntitle: T\nroot: README.md\n");
        std::fs::create_dir_all(dir.join("docs/guides/deep")).unwrap();
        std::fs::write(dir.join("docs/guides/a.md"), "# A").unwrap();
        let _watch = watching(&dir).await;
        // `rm -rf docs/guides && git checkout docs/guides`
        std::fs::remove_dir_all(dir.join("docs/guides")).unwrap();
        std::fs::create_dir_all(dir.join("docs/guides/deep")).unwrap();
        std::fs::write(dir.join("docs/guides/a.md"), "# A").unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        for later in ["docs/guides/later.md", "docs/guides/deep/later.md"] {
            let later = dir.join(later);
            std::fs::write(&later, "# Later").unwrap();
            assert!(reported(&later, 0).await, "{}", later.display());
        }
    }

    #[tokio::test]
    async fn the_comment_store_is_reported_and_the_rest_of_devpage_is_not() {
        let dir = served("store", "kind: site\ntitle: T\nroot: README.md\n");
        std::fs::create_dir_all(dir.join(".devpage/comments/docs/README.md")).unwrap();
        std::fs::create_dir_all(dir.join(".devpage/cache")).unwrap();
        let _watch = watching(&dir).await;
        write_in(&dir.join(".devpage/cache"));
        let thread = dir.join(".devpage/comments/docs/README.md/20261025T000000Z-abcd.yml");
        std::fs::write(&thread, "thread: []\n").unwrap();
        assert!(reported(&thread, 0).await);
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(raw(&dir.join(".devpage/cache")), 0);
    }

    #[tokio::test]
    async fn an_ignored_folder_a_watch_pattern_names_is_watched() {
        let dir = served(
            "build",
            "kind: site\ntitle: T\nroot: README.md\nwatch: [\"build/*.svg\"]\n",
        );
        std::fs::write(dir.join("docs/.gitignore"), "build/\nout/\n").unwrap();
        std::fs::create_dir_all(dir.join("out")).unwrap();
        std::fs::create_dir_all(dir.join("docs/out")).unwrap();
        let _watch = watching(&dir).await;
        let chart = dir.join("docs/build/chart.svg");
        std::fs::write(&chart, "<svg/>").unwrap();
        assert!(reported(&chart, 0).await);
        // `docs/out/` is ignored too, and no pattern reaches into it.
        write_in(&dir.join("docs/out"));
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(raw(&dir.join("docs/out")), 0);
    }

    /// The listeners of the served folder `dir`, started on `docs/`.
    fn listeners(dir: &Path, changes: Changes) -> Listeners {
        let document = Arc::new(Document::load(&dir.join("docs"), dir).unwrap());
        Listeners {
            served: ServedFolder::canonical(dir.to_path_buf()),
            entry: Arc::new(Entry::new(document)),
            index: Arc::new(Index::build(dir)),
            designs: Designs::new(dir.to_path_buf()),
            changes,
            scope: Scope::root(),
            threads: Arc::new(Yaml::new(dir.canonicalize().unwrap())),
        }
    }

    /// The event a platform watcher reports when `paths` were written.
    fn written(paths: &[PathBuf]) -> notify::Result<notify::Event> {
        let mut event = notify::Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Any)));
        event.paths = paths.to_vec();
        Ok(event)
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
        let listeners = listeners(&dir, Changes::default());
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
        let (heard, _) = listeners.forget(&listeners.sift(written(&noise)));
        assert_eq!(CANONICALIZED.with(|count| count.get()), before);
        assert_eq!(heard, []);
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
        let listeners = listeners(&dir, Changes::default());
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
        let (heard, _) = listeners.forget(&arrived);
        assert_eq!(
            heard,
            [Change::Comments("/docs/README.md".into()), Change::Refresh]
        );
    }

    #[test]
    fn a_new_gitignore_rule_takes_effect_without_a_restart() {
        let dir = served("rule", "kind: site\ntitle: T\nroot: README.md\n");
        std::fs::create_dir_all(dir.join("docs/drafts")).unwrap();
        let listeners = listeners(&dir, Changes::default());
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
        let mut listeners = listeners(&dir, Changes::default());
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
        let changes = Changes::default();
        let mut received = changes.subscribe(Scope::root());
        let handler = intake(listeners(&dir, changes), Arc::default());
        // Saves trickling in, as an editor or a build writes, within one burst.
        for _ in 0..5 {
            handler(written(&[dir.join("docs/README.md")]));
            std::thread::sleep(BURST / 25);
        }
        std::thread::sleep(BURST * 6);
        assert_eq!(received.drain(), [Change::Refresh]);

        // A later burst is a new one.
        handler(written(&[dir.join("docs/README.md")]));
        std::thread::sleep(BURST * 6);
        assert_eq!(received.drain(), [Change::Refresh]);
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
        let threads = Arc::new(Yaml::new(dir.canonicalize().unwrap()));
        after_with(dir, threads, |_| touch()).await
    }

    /// The messages that arrive within a short while after `touch` runs,
    /// with `threads` as the comment store the server writes through; `touch`
    /// is given the bus, as a server would hold it.
    async fn after_with(
        dir: &Path,
        threads: Arc<Yaml>,
        touch: impl FnOnce(&Changes),
    ) -> Vec<String> {
        let document = Arc::new(Document::load(&dir.join("docs"), dir).unwrap());
        let index = Arc::new(Index::build(dir));
        let changes = Changes::default();
        let mut received = changes.subscribe(Scope::root());
        let designs = Designs::new(dir.to_path_buf());
        let _watcher = start(
            Arc::new(Entry::new(document)),
            index,
            designs,
            changes.clone(),
            threads,
        )
        .unwrap();
        // Let the platform's watcher settle before changing anything.
        tokio::time::sleep(Duration::from_millis(300)).await;
        touch(&changes);
        let mut messages = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_millis(1500);
        while let Ok(Some(change)) = tokio::time::timeout_at(deadline, received.next()).await {
            messages.push(change.message());
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
            served: ServedFolder::canonical(dir.clone()),
            entry: Arc::new(Entry::new(document)),
            index: Arc::new(Index::build(&dir)),
            designs: designs.clone(),
            changes: Changes::default(),
            scope: Scope::root(),
            threads: Arc::new(Yaml::new(dir.canonicalize().unwrap())),
        };
        assert!(designs.resolve("docs/screen.design").is_ok());
        assert!(designs.resolve(".devpage/screen.design").is_ok());

        // Saved broken: the store answers from memory until it is told.
        std::fs::write(dir.join("docs/screen.design"), broken).unwrap();
        std::fs::write(dir.join(".devpage/screen.design"), broken).unwrap();
        assert!(designs.resolve("docs/screen.design").is_ok());

        listeners.forget(&[
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
    async fn a_thread_written_through_the_store_is_announced_once_and_a_hand_edit_once() {
        let dir = served("announced", "kind: site\ntitle: T\nroot: README.md\n");
        let threads = Arc::new(Yaml::new(dir.canonicalize().unwrap()));
        let store = threads.clone();
        let messages = after_with(&dir, threads, |changes| {
            let comments = crate::comments::Comments::new(store, changes.clone(), Scope::root());
            let anchor = crate::comments::Anchor {
                quote: "x".into(),
                prefix: String::new(),
                suffix: String::new(),
            };
            let id = comments
                .create("/docs/README.md", anchor, "Ana", "hi")
                .unwrap();
            // Then, by hand, a reply appended to the same thread file.
            let file = dir.join(format!(".devpage/comments/docs/README.md/{id}.yml"));
            std::thread::sleep(Duration::from_millis(200));
            let text = std::fs::read_to_string(&file).unwrap();
            std::fs::write(&file, text + "- author: Bo\n  at: x\n  text: by hand\n").unwrap();
        })
        .await;
        let comments: Vec<_> = messages
            .iter()
            .filter(|m| *m == "comments /docs/README.md")
            .collect();
        assert_eq!(comments.len(), 2, "the write, then the edit: {messages:?}");
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
        let changes = Changes::default();
        let mut received = changes.subscribe(Scope::root());
        let designs = Designs::new(dir.clone());
        let threads = Arc::new(Yaml::new(&dir));
        let _watcher = start(entry.clone(), index, designs, changes, threads).unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Its own file changing refreshes pages, though `watch` names only Markdown.
        std::fs::write(
            dir.join("docs/devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\nwatch: [\"**/*.svg\"]\n",
        )
        .unwrap();
        let drain = |received: &mut crate::changes::Subscription| {
            received
                .drain()
                .iter()
                .map(Change::message)
                .collect::<Vec<_>>()
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
