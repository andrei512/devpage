//! Watches the served folder: the platform's file events, sifted as they
//! arrive and gathered into bursts, each handed to the core's
//! [`Listeners`], which decide everything that follows from it. Only the
//! folders the folder says to watch are watched, through the platform's
//! watcher: `macos.rs`, `inotify.rs`, or `recursive.rs` elsewhere.

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
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use anyhow::Result;
use devpage_core::watch::Listeners;
use notify::event::{CreateKind, EventKind, RemoveKind};

use devpage_core::folder::{Folder, WatchSet};

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

/// Starts watching the folders `listeners`' folder says to watch, and hands
/// them every burst of changed paths. A path that can't matter — under
/// `.git`, or excluded by a `.gitignore` and not named by its site's
/// `watch` — is dropped as it arrives, before the listeners hear of it.
pub fn start(listeners: Listeners) -> Result<Watch> {
    let root = listeners.folder().served().path().to_path_buf();
    let set = listeners.folder().watched();
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
/// they arrive and passes the survivors to a thread that hands them to
/// `listeners` in bursts, re-planning the watch set when a burst changes
/// it. The thread ends when the handler is dropped.
fn intake(listeners: Listeners, watching: Watching) -> Handler {
    let listeners = Arc::new(listeners);
    let (arrivals, arriving) = mpsc::channel::<Arrival>();
    let handler: Handler = {
        let listeners = listeners.clone();
        Arc::new(move |event: notify::Result<notify::Event>| {
            #[cfg(test)]
            tests::record(&event);
            let folder = listeners.folder();
            if let Ok(event) = &event {
                for hidden in hidden(folder, event) {
                    let _ = arrivals.send(Arrival::Hidden(hidden));
                }
            }
            for path in sift(folder, event) {
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
            let reshaped = changed(&listeners, &paths);
            if let Some(platform) = watching.get().and_then(Weak::upgrade) {
                let mut platform = platform.lock().unwrap_or_else(|p| p.into_inner());
                if hidden || reshaped || reaches_past(&platform, &paths) {
                    replan(&listeners, &mut platform);
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

/// The paths of `event` that can matter, as `folder` judges them from the
/// path alone. Accesses and errors carry none.
fn sift(folder: &Folder, event: notify::Result<notify::Event>) -> Vec<PathBuf> {
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
        .filter(|path| folder.matters(path, is_dir))
        .collect()
}

/// The folders `event` says were created out of sight directly in a folder
/// in sight — a build's first `target/` — judged from the path alone, as
/// sifting is.
fn hidden(folder: &Folder, event: &notify::Event) -> Vec<PathBuf> {
    if event.kind != EventKind::Create(CreateKind::Folder) {
        return Vec::new();
    }
    let served = folder.served();
    event
        .paths
        .iter()
        .filter(|path| {
            !folder.matters(path, true)
                && path
                    .parent()
                    .is_some_and(|parent| served.contains(parent) && folder.matters(parent, true))
        })
        .cloned()
        .collect()
}

/// Has the platform watch the folders to watch, as the folder plans them
/// now. What is already in a folder the platform didn't report on before
/// is handled as changed — a thread written into a new comment folder
/// before its watch started still reaches its page.
fn replan(listeners: &Listeners, platform: &mut Platform) {
    let folder = listeners.folder();
    let mut found = Vec::new();
    for (watched, deep) in platform.apply(folder.watched()) {
        within(&watched, deep, &mut found);
        found.push(watched);
    }
    found.retain(|path| folder.matters(path, path.is_dir()));
    changed(listeners, &found);
}

/// Hands `listeners` a burst of sifted `paths`, canonical, and says
/// whether the folders to watch may differ now.
fn changed(listeners: &Listeners, paths: &[PathBuf]) -> bool {
    let paths: Vec<PathBuf> = paths.iter().map(|path| canonical(path)).collect();
    listeners.changed(&paths)
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
    use crate::adapters::changes::{Change, Changes};
    use devpage_core::comments::Comments;
    use devpage_core::designs::Designs;
    use devpage_core::document::Document;
    use devpage_core::folder::ServedFolder;
    use devpage_core::widgets::Modules;
    use notify::event::{DataChange, ModifyKind};

    thread_local! {
        /// How many paths this thread has canonicalized.
        pub static CANONICALIZED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    /// Every path a handler was handed, by any watcher in any test, before
    /// sifting: what the platform reported.
    static RAW: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

    /// Every path a handler was told was removed, by any watcher in any test.
    static REMOVED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

    pub fn record(event: &notify::Result<notify::Event>) {
        if let Ok(event) = event {
            RAW.lock().unwrap().extend(event.paths.iter().cloned());
            if event.kind.is_remove() {
                REMOVED.lock().unwrap().extend(event.paths.iter().cloned());
            }
        }
    }

    /// Whether the platform has reported `path` removed: by then it has
    /// dropped whatever watch was on it.
    fn removed(path: &Path) -> bool {
        REMOVED
            .lock()
            .unwrap()
            .iter()
            .any(|removed| removed == path)
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
        let watch = start(listeners(dir, &Changes::default()).0).unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        watch
    }

    /// Waits until the platform has reported something in `folder` since
    /// `before`, and says whether it did: at once when it does, within ten
    /// seconds however loaded the machine.
    async fn reported(folder: &Path, before: usize) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if raw(folder) > before {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    }

    /// Waits until `watch`'s platform watcher is as `ready` says — the
    /// re-plan a change set off has run — and says whether it was within
    /// ten seconds. What a test does next is then reported, or not, by the
    /// watches the change made, never by those it replaces.
    async fn planned(watch: &Watch, ready: impl Fn(&Platform) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if ready(&watch._platform.lock().unwrap()) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
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
        let watch = watching(&dir).await;

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
        let plan = dir.join("plan");
        assert!(planned(&watch, |p| p.live(&plan)).await, "plan/ watched");
        let page = dir.join("plan/design.md");
        std::fs::write(&page, "# Plan").unwrap();
        assert!(reported(&page, 0).await, "a file in a new folder");
    }

    #[tokio::test]
    async fn a_new_gitignore_rule_stops_a_folders_events() {
        let dir = served("gen", "kind: site\ntitle: T\nroot: README.md\n");
        std::fs::write(dir.join(".gitignore"), "/target/\n").unwrap();
        std::fs::create_dir(dir.join("gen")).unwrap();
        let watch = watching(&dir).await;
        write_in(&dir.join("gen"));
        assert!(reported(&dir.join("gen/f19"), 0).await);

        std::fs::write(dir.join(".gitignore"), "/target/\n/gen/\n").unwrap();
        let generated = dir.join("gen");
        assert!(
            planned(&watch, |p| !p.covers(&generated)
                && !p.watches_within(&generated))
            .await
        );
        let before = raw(&generated);
        write_in(&generated);
        // Something in sight saved after, so the quiet is the platform's.
        let page = dir.join("docs/README.md");
        let heard = raw(&page);
        std::fs::write(&page, "# Changed").unwrap();
        assert!(reported(&page, heard).await);
        assert_eq!(raw(&generated), before);
    }

    #[tokio::test]
    async fn a_nested_build_folder_stops_its_events_once_made() {
        let dir = served("crate", "kind: site\ntitle: T\nroot: README.md\n");
        std::fs::create_dir_all(dir.join("crate/src")).unwrap();
        std::fs::write(dir.join("crate/.gitignore"), "target/\n").unwrap();
        let watch = watching(&dir).await;
        // `cargo build` in the crate: its first writes may still be reported.
        write_in(&dir.join("crate/target/debug"));
        let target = dir.join("crate/target");
        assert!(planned(&watch, |p| !p.covers(&target)).await);
        let before = raw(&target);
        write_in(&dir.join("crate/target/debug"));
        write_in(&dir.join("crate/target/release"));
        // The rest of the crate is still watched, and heard after them.
        std::fs::write(dir.join("crate/src/lib.rs"), "").unwrap();
        assert!(reported(&dir.join("crate/src/lib.rs"), 0).await);
        assert_eq!(raw(&target), before);
    }

    #[tokio::test]
    async fn a_folder_deleted_and_made_again_in_one_burst_is_watched_again() {
        let dir = served("again", "kind: site\ntitle: T\nroot: README.md\n");
        std::fs::write(dir.join(".gitignore"), "/crate/target/\n").unwrap();
        // `crate/` holds something out of sight, so it is watched shallow.
        std::fs::create_dir_all(dir.join("crate/target")).unwrap();
        std::fs::write(dir.join("crate/a.md"), "# A").unwrap();
        let watch = watching(&dir).await;
        std::fs::remove_dir_all(dir.join("crate")).unwrap();
        std::fs::create_dir_all(dir.join("crate/target")).unwrap();
        let folder = dir.join("crate");
        assert!(
            planned(&watch, |p| removed(&folder) && p.live(&folder)).await,
            "crate/ watched again"
        );
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
        let watch = watching(&dir).await;
        // `rm -rf docs/guides && git checkout docs/guides`
        std::fs::remove_dir_all(dir.join("docs/guides")).unwrap();
        std::fs::create_dir_all(dir.join("docs/guides/deep")).unwrap();
        std::fs::write(dir.join("docs/guides/a.md"), "# A").unwrap();
        let (guides, deep) = (dir.join("docs/guides"), dir.join("docs/guides/deep"));
        assert!(
            planned(&watch, |p| removed(&guides)
                && p.live(&guides)
                && p.live(&deep))
            .await
        );
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
            Arc::new(devpage_core::comments::yaml::Yaml::new(served)),
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

    /// The listeners of the served folder `dir`, opened on `docs/`,
    /// publishing on `changes` with no search index — with the design store
    /// and the comments they hold, as a server would share them.
    fn listeners(dir: &Path, changes: &Changes) -> (Listeners, Designs, Comments) {
        let folder = opened(dir);
        let designs = Designs::new(folder.served());
        let threads = comments(folder.served(), changes);
        let listeners = Listeners::new(
            folder,
            designs.clone(),
            Modules::default(),
            Some(threads.clone()),
            Arc::new(changes.clone()),
            None,
        );
        (listeners, designs, threads)
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
        let changes = Changes::default();
        let mut received = changes.subscribe();
        let (listeners, designs, _) = listeners(&dir, &changes);
        assert!(designs.resolve("docs/build/screen.design").is_ok());
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
        let folder = listeners.folder();
        assert_eq!(sift(folder, written(&noise)), Vec::<PathBuf>::new());
        let mut created = notify::Event::new(EventKind::Create(CreateKind::Folder));
        created.paths = vec![dir.join("docs/build")];
        assert_eq!(sift(folder, Ok(created)), Vec::<PathBuf>::new());

        let before = CANONICALIZED.with(|count| count.get());
        changed(&listeners, &sift(folder, written(&noise)));
        assert_eq!(CANONICALIZED.with(|count| count.get()), before);
        assert_eq!(received.drain(), []);
        assert!(designs.resolve("docs/build/screen.design").is_ok());
    }

    #[test]
    fn comment_threads_and_files_watch_names_still_arrive() {
        let dir = served(
            "named",
            "kind: site\ntitle: T\nroot: README.md\nwatch: [\"**/*.md\", \"build/*.svg\"]\n",
        );
        let changes = Changes::default();
        let mut received = changes.subscribe();
        let (listeners, _, _) = listeners(&dir, &changes);
        let thread = dir.join(".devpage/comments/docs/README.md/20261025T000000Z-abcd.yml");
        let named = dir.join("docs/build/chart.svg");
        let unnamed = dir.join("docs/build/chart.png");
        let page = dir.join("docs/README.md");
        let arrived = sift(
            listeners.folder(),
            written(&[thread.clone(), named.clone(), unnamed, page.clone()]),
        );
        assert_eq!(arrived, vec![thread, named, page]);
        changed(&listeners, &arrived);
        assert_eq!(
            received.drain(),
            [Change::Comments("/docs/README.md".into()), Change::Refresh]
        );
    }

    #[test]
    fn a_new_gitignore_rule_takes_effect_without_a_restart() {
        let dir = served("rule", "kind: site\ntitle: T\nroot: README.md\n");
        std::fs::create_dir_all(dir.join("docs/drafts")).unwrap();
        let (listeners, _, _) = listeners(&dir, &Changes::default());
        let folder = listeners.folder();
        let draft = dir.join("docs/drafts/idea.md");
        assert_eq!(
            sift(folder, written(std::slice::from_ref(&draft))),
            vec![draft.clone()]
        );

        std::fs::write(dir.join("docs/.gitignore"), "build/\ndrafts/\n").unwrap();
        let rule = dir.join("docs/.gitignore");
        changed(&listeners, &sift(folder, written(&[rule])));
        assert_eq!(sift(folder, written(&[draft])), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_burst_holds_each_arrival_once_in_the_order_it_first_arrived() {
        let (arrivals, arriving) = mpsc::channel();
        for arrival in ["a", "b", "a", "a", "c", "b"] {
            arrivals.send(arrival).unwrap();
        }
        assert_eq!(next_burst(&arriving), Some(vec!["a", "b", "c"]));
        drop(arrivals);
        assert_eq!(next_burst(&arriving), None);
    }

    #[tokio::test]
    async fn a_burst_of_changes_to_one_file_sends_one_change() {
        let dir = served("burst", "kind: site\ntitle: T\nroot: README.md\n");
        let changes = Changes::default();
        let mut received = changes.subscribe();
        let (listeners, _, _) = listeners(&dir, &changes);
        let page = dir.join("docs/README.md");
        // Saves trickling in, as an editor or a build writes: one burst.
        let (arrivals, arriving) = mpsc::channel();
        for _ in 0..5 {
            arrivals.send(page.clone()).unwrap();
        }
        let burst = next_burst(&arriving).unwrap();
        changed(&listeners, &burst);
        assert_eq!(received.drain(), [Change::Refresh]);
        // A later burst is a new one.
        changed(&listeners, std::slice::from_ref(&page));
        assert_eq!(received.drain(), [Change::Refresh]);
        // And what the platform's watcher reports is handed on in bursts.
        let handler = intake(listeners, Arc::default());
        handler(written(&[page]));
        let heard = tokio::time::timeout(Duration::from_secs(10), received.next()).await;
        assert_eq!(heard.ok().flatten(), Some(Change::Refresh));
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
        let changes = Changes::default();
        let mut received = changes.subscribe();
        let (listeners, _, threads) = listeners(dir, &changes);
        let _watcher = start(listeners).unwrap();
        // Let the platform's watcher settle before changing anything.
        tokio::time::sleep(Duration::from_millis(300)).await;
        touch(&threads);
        listen(&mut received, within, want).await
    }

    /// What `received` hears within `within`, stopping early at the first
    /// message that is `want`.
    async fn listen(
        received: &mut crate::adapters::changes::Subscription,
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
        let changes = Changes::default();
        let mut received = changes.subscribe();
        let (listeners, _, comments) = listeners(&dir, &changes);
        let _watcher = start(listeners).unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        let announced = "comments /docs/README.md";
        let anchor = devpage_core::comments::Anchor {
            quote: "x".into(),
            prefix: String::new(),
            suffix: String::new(),
        };
        let id = comments
            .create("/docs/README.md", anchor, "Ana", "hi")
            .unwrap();
        let loud = Duration::from_secs(10);
        let write = listen(&mut received, loud, Some(announced)).await;
        assert_eq!(
            write.last().map(String::as_str),
            Some(announced),
            "the write"
        );
        // Then, by hand, a reply appended to the same thread file — however
        // the watcher's bursts fall around the store's write and this edit.
        let file = dir.join(format!(".devpage/comments/docs/README.md/{id}.yml"));
        let text = std::fs::read_to_string(&file).unwrap();
        std::fs::write(&file, text + "- author: Bo\n  at: x\n  text: by hand\n").unwrap();
        let edit = listen(&mut received, loud, Some(announced)).await;
        assert_eq!(edit.last().map(String::as_str), Some(announced), "the edit");
        let rest = listen(&mut received, Duration::from_millis(1500), None).await;
        assert!(!rest.iter().any(|m| m == announced), "once each: {rest:?}");
    }

    #[tokio::test]
    async fn a_comment_thread_always_reaches_its_page() {
        let dir = served(
            "comments",
            "kind: site\ntitle: T\nroot: README.md\nwatch: [\"**/*.md\"]\n",
        );
        let changes = Changes::default();
        let mut received = changes.subscribe();
        let _watcher = start(listeners(&dir, &changes).0).unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        let thread = dir.join(".devpage/comments/docs/README.md");
        std::fs::create_dir_all(&thread).unwrap();
        std::fs::write(thread.join("20261025T000000Z-abcd.yml"), "thread: []\n").unwrap();
        let announced = "comments /docs/README.md";
        let mut messages = listen(&mut received, Duration::from_secs(10), Some(announced)).await;
        assert!(messages.iter().any(|m| m == announced), "{messages:?}");
        messages.extend(listen(&mut received, Duration::from_millis(1500), None).await);
        assert!(!messages.iter().any(|m| m == "change"), "{messages:?}");
    }

    #[tokio::test]
    async fn the_watch_list_follows_the_entry_file() {
        let dir = served(
            "relist",
            "kind: site\ntitle: T\nroot: README.md\nwatch: [\"**/*.md\"]\n",
        );
        let changes = Changes::default();
        let mut received = changes.subscribe();
        let _watcher = start(listeners(&dir, &changes).0).unwrap();
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
