//! Watches the served folder: tells the folder what changed and publishes
//! what it says open pages should hear, tells them a comment thread changed,
//! and tells the design store, the widget modules and the search index every
//! file that changed and isn't out of sight. Only the folders the folder says
//! to watch are watched, through the platform's watcher: `macos.rs`,
//! `inotify.rs`, or `recursive.rs` elsewhere.

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

use crate::changes::{Change, Changes};
use crate::comments::Comments;
use crate::designs::Designs;
use crate::folder::{Folder, WatchSet};
use crate::search::Search;
use crate::widgets::Modules;
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
/// change bus; a change to a comment thread publishes what `threads`, the
/// comments the server writes through, heard of it, whatever the site's
/// `watch` says — nothing for a write of its own, which was announced as it
/// landed, and nothing at all with comments off (`threads` is `None`).
/// A path that can't matter — under `.git`, or excluded by a `.gitignore` and
/// not named by its site's `watch` — is dropped as it arrives, before any
/// listener hears of it. Every other changed file outside `.devpage/` also
/// goes to the design store, the widget modules and the search index,
/// whatever `watch` says, since any file may be part of a design.
pub fn start(
    folder: Arc<Folder>,
    modules: Modules,
    search: Option<Arc<Search>>,
    designs: Designs,
    changes: Changes,
    threads: Option<Comments>,
) -> Result<Watch> {
    let root = folder.served().path().to_path_buf();
    let set = folder.watched();
    let listeners = Listeners {
        folder,
        modules,
        search,
        designs,
        changes,
        threads,
    };
    let watching = Arc::new(OnceLock::new());
    let mut platform = Platform::new(&root, intake(listeners, watching.clone()))?;
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
            let reshaped = listeners.changed(&paths);
            if let Some(platform) = watching.get().and_then(Weak::upgrade) {
                let mut platform = platform.lock().unwrap_or_else(|p| p.into_inner());
                if hidden || reshaped || reaches_past(&platform, &paths) {
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

/// Who hears about files that change: the served folder, which follows
/// them and says what open pages should hear; the caches that forget what
/// changed — the widget modules, the design store and the search index —
/// and, through the change bus, open pages.
struct Listeners {
    folder: Arc<Folder>,
    modules: Modules,
    /// The search index, when this server answers searches.
    search: Option<Arc<Search>>,
    designs: Designs,
    changes: Changes,
    /// The comments the server writes through, which know the thread files
    /// they wrote — and announced — themselves; `None` with comments off.
    threads: Option<Comments>,
}

impl Listeners {
    /// The paths of `event` that can matter to a listener, as the folder
    /// judges them from the path alone. Accesses and errors carry none.
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
            .filter(|path| self.folder.matters(path, is_dir))
            .collect()
    }

    /// The folders `event` says were created out of sight directly in a
    /// folder in sight — a build's first `target/` — judged from the path
    /// alone, as sifting is.
    fn hidden(&self, event: &notify::Event) -> Vec<PathBuf> {
        if event.kind != EventKind::Create(CreateKind::Folder) {
            return Vec::new();
        }
        let served = self.folder.served();
        event
            .paths
            .iter()
            .filter(|path| {
                !self.folder.matters(path, true)
                    && path.parent().is_some_and(|parent| {
                        served.contains(parent) && self.folder.matters(parent, true)
                    })
            })
            .cloned()
            .collect()
    }

    /// Has the platform watch the folders to watch, as the folder plans them
    /// now. What is already in a folder the platform didn't report on before
    /// is handled as changed — a thread written into a new comment folder
    /// before its watch started still reaches its page.
    fn replan(&self, platform: &mut Platform) {
        let mut found = Vec::new();
        for (folder, deep) in platform.apply(self.folder.watched()) {
            within(&folder, deep, &mut found);
            found.push(folder);
        }
        found.retain(|path| self.folder.matters(path, path.is_dir()));
        self.changed(&found);
    }

    /// `paths` changed, were created or were deleted: every cache forgets
    /// what it read of them, open pages hear what they should show — at most
    /// one refresh — and then the search index reads them again. Says whether
    /// the folders to watch may differ now.
    fn changed(&self, paths: &[PathBuf]) -> bool {
        let (heard, searched, reshaped) = self.forget(paths);
        for change in heard {
            self.changes.publish(change);
        }
        // After the pages hear, so indexing text never holds up a refresh.
        if let Some(search) = &self.search {
            search.changed(&self.folder, &searched);
        }
        reshaped
    }

    /// The caches forget what they read of `paths`, and the folder follows
    /// them; the changes open pages should hear come back — a page's
    /// comments as each thread changes, then what the folder says — with the
    /// paths the search index should read again, and whether the folders to
    /// watch may differ. Nothing is published.
    fn forget(&self, paths: &[PathBuf]) -> (Vec<Change>, Vec<PathBuf>, bool) {
        let paths: Vec<PathBuf> = paths.iter().map(|path| canonical(path)).collect();
        // A thread changed: the pages showing its page fetch comments —
        // unless the store's own write was announced already.
        let mut heard = self
            .threads
            .as_ref()
            .map(|threads| threads.heard(&paths))
            .unwrap_or_default();
        let mut searched = Vec::new();
        let served = self.folder.served();
        for path in paths {
            let Some(relative) = served.relative(&path) else {
                continue;
            };
            if relative.starts_with(".devpage") {
                continue;
            }
            self.designs.changed(&path);
            self.modules.forget(&path);
            searched.push(path);
        }
        let folder = self.folder.changed(&searched);
        heard.extend(folder.changes);
        (heard, searched, folder.reshaped)
    }
}

/// Whether the burst of `paths` reaches past what the platform watches: a
/// folder it doesn't report on appeared, or one holding a watched folder went
/// away.
fn reaches_past(platform: &Platform, paths: &[PathBuf]) -> bool {
    paths.iter().any(|path| {
        if path.is_dir() {
            !platform.covers(path)
        } else {
            platform.watches_within(path)
        }
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::folder::ServedFolder;
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
        let folder = opened(dir);
        let watch = start(
            folder.clone(),
            Modules::default(),
            None,
            Designs::new(folder.served()),
            Changes::default(),
            Some(comments(folder.served(), &Changes::default())),
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

    /// The comments of `served`, in its thread files, announced on `changes`.
    fn comments(served: &ServedFolder, changes: &Changes) -> Comments {
        Comments::new(
            Arc::new(crate::comments::yaml::Yaml::new(served)),
            Arc::new(changes.clone()),
            served.clone(),
        )
    }

    /// The served folder `dir`, opened on `docs/`.
    fn opened(dir: &Path) -> Arc<Folder> {
        let served = ServedFolder::new(dir).unwrap();
        let document = Arc::new(Document::load(&dir.join("docs"), &served).unwrap());
        Arc::new(Folder::new(document))
    }

    /// The listeners of the served folder `dir`, started on `docs/`.
    fn listeners(dir: &Path, changes: Changes) -> Listeners {
        let folder = opened(dir);
        Listeners {
            designs: Designs::new(folder.served()),
            threads: Some(comments(folder.served(), &changes)),
            folder,
            modules: Modules::default(),
            search: None,
            changes,
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
        let (heard, _, _) = listeners.forget(&listeners.sift(written(&noise)));
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
        let (heard, _, _) = listeners.forget(&arrived);
        assert_eq!(
            heard,
            [Change::Comments("/docs/README.md".into()), Change::Refresh]
        );
    }

    #[test]
    fn with_comments_off_a_thread_file_announces_nothing() {
        let dir = served("off", "kind: site\ntitle: T\nroot: README.md\n");
        let listeners = Listeners {
            threads: None,
            ..listeners(&dir, Changes::default())
        };
        let thread = dir.join(".devpage/comments/docs/README.md/20261025T000000Z-abcd.yml");
        let arrived = listeners.sift(written(std::slice::from_ref(&thread)));
        assert_eq!(arrived, vec![thread]);
        let (heard, _, _) = listeners.forget(&arrived);
        assert_eq!(heard, []);
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
        listeners.search = Some(Arc::new(Search::build(
            &listeners.folder,
            &dir.join("docs/devpage.yml"),
        )));
        let found = |word: &str| {
            let search = listeners.search.as_ref().unwrap();
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
        let mut received = changes.subscribe();
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

    /// The messages that arrive within 1.5 s after `touch` runs: long enough
    /// for any it causes, so a test that expects none waits it out.
    async fn after(dir: &Path, touch: impl FnOnce()) -> Vec<String> {
        heard(dir, |_| touch(), Duration::from_millis(1500), None).await
    }

    /// The messages that arrive after `touch` runs, up to and including the
    /// first that is `want`, for up to 10 s — a test that expects one goes on
    /// the moment it arrives.
    async fn until(dir: &Path, want: &str, touch: impl FnOnce()) -> Vec<String> {
        heard(dir, |_| touch(), Duration::from_secs(10), Some(want)).await
    }

    /// What arrives after `touch` runs; `touch` is given the comments the
    /// watcher hears thread files through, as a server would hold them.
    async fn heard(
        dir: &Path,
        touch: impl FnOnce(&Comments),
        within: Duration,
        want: Option<&str>,
    ) -> Vec<String> {
        let folder = opened(dir);
        let changes = Changes::default();
        let mut received = changes.subscribe();
        let designs = Designs::new(folder.served());
        let threads = comments(folder.served(), &changes);
        let _watcher = start(
            folder,
            Modules::default(),
            None,
            designs,
            changes.clone(),
            Some(threads.clone()),
        )
        .unwrap();
        // Let the platform's watcher settle before changing anything.
        tokio::time::sleep(Duration::from_millis(300)).await;
        touch(&threads);
        listen(&mut received, within, want).await
    }

    /// What `received` hears within `within`, stopping early at the first
    /// message that is `want`.
    async fn listen(
        received: &mut crate::changes::Subscription,
        within: Duration,
        want: Option<&str>,
    ) -> Vec<String> {
        let mut messages = Vec::new();
        let deadline = tokio::time::Instant::now() + within;
        while let Ok(Some(change)) = tokio::time::timeout_at(deadline, received.next()).await {
            let message = change.message();
            let done = want == Some(message.as_str());
            messages.push(message);
            if done {
                break;
            }
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
        let listeners = listeners(&dir, Changes::default());
        let designs = listeners.designs.clone();
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
        let messages = until(&dir, "change", || {
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
        let messages = heard(
            &dir,
            |comments| {
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
            },
            Duration::from_millis(1500),
            None,
        )
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
        let folder = opened(&dir);
        let changes = Changes::default();
        let mut received = changes.subscribe();
        let designs = Designs::new(folder.served());
        let threads = comments(folder.served(), &changes);
        let _watcher = start(
            folder,
            Modules::default(),
            None,
            designs,
            changes,
            Some(threads),
        )
        .unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Its own file changing refreshes pages, though `watch` names only Markdown.
        std::fs::write(
            dir.join("docs/devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\nwatch: [\"**/*.svg\"]\n",
        )
        .unwrap();
        let loud = Duration::from_secs(10);
        let messages = listen(&mut received, loud, Some("change")).await;
        assert!(messages.iter().any(|m| m == "change"), "{messages:?}");

        // Now Markdown is outside `watch`, and an SVG is in it.
        std::fs::write(dir.join("docs/README.md"), "# Changed").unwrap();
        let messages = listen(&mut received, Duration::from_millis(800), None).await;
        assert!(!messages.iter().any(|m| m == "change"), "{messages:?}");
        std::fs::write(dir.join("docs/logo.svg"), "<svg/>").unwrap();
        let messages = listen(&mut received, loud, Some("change")).await;
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
        let loud = until(&dir, "change", || {
            std::fs::write(dir.join("other/notes.txt"), "x").unwrap()
        })
        .await;
        assert!(loud.iter().any(|m| m == "change"), "{loud:?}");
        // The entry's own pages still refresh, and so does a doc in no site.
        let entry = until(&dir, "change", || {
            std::fs::write(dir.join("docs/README.md"), "# Changed").unwrap()
        })
        .await;
        assert!(entry.iter().any(|m| m == "change"), "{entry:?}");
        std::fs::create_dir_all(dir.join("plan")).unwrap();
        let loose = until(&dir, "change", || {
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
        let messages = until(&dir, "change", || {
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
        let loud = until(&dir, "change", || {
            std::fs::write(dir.join("docs/widgets/format.ts"), "export const f = 1;").unwrap()
        })
        .await;
        assert!(loud.iter().any(|m| m == "change"), "{loud:?}");
    }
}
