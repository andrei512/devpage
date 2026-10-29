//! Local mode's response to saved files, tested without a platform watcher:
//! who hears a burst, and in what order. Ported from the watcher, so the
//! order is pinned on every platform.

use super::*;
use crate::changes::{Change, Recorder};
use crate::document::Document;
use crate::folder::ServedFolder;
use std::path::Path;
use std::sync::Mutex;

/// A served folder `dir` with a site in `docs/`, its `build/` ignored.
fn served(name: &str, site: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("devpage-core-watch-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("docs/build")).unwrap();
    std::fs::write(dir.join("docs/README.md"), "# Home").unwrap();
    std::fs::write(dir.join("docs/devpage.yml"), site).unwrap();
    std::fs::write(dir.join("docs/.gitignore"), "build/\n").unwrap();
    dir.canonicalize().unwrap()
}

/// The served folder `dir`, opened on `docs/`.
fn opened(dir: &Path) -> Arc<Folder> {
    let served = ServedFolder::new(dir).unwrap();
    let document = Arc::new(Document::load(&dir.join("docs"), &served).unwrap());
    Arc::new(Folder::new(document))
}

/// The comments of `served`, in its thread files, announced on `changes`.
fn comments(served: &ServedFolder, changes: Arc<dyn Publish>) -> Comments {
    Comments::new(
        Arc::new(crate::comments::yaml::Yaml::new(served)),
        changes,
        served.clone(),
    )
}

/// The listeners of the served folder `dir`, publishing on `changes`, with
/// the search index when `search` is given.
fn listeners(dir: &Path, changes: Arc<dyn Publish>, search: bool) -> Listeners {
    let folder = opened(dir);
    let search = search.then(|| Arc::new(Search::build(&folder, &dir.join("docs/devpage.yml"))));
    Listeners::new(
        folder.clone(),
        Designs::new(folder.served()),
        Modules::default(),
        Some(comments(folder.served(), changes.clone())),
        changes,
        search,
    )
}

/// The addresses of the sections `search` finds for `word`.
fn found(search: &Search, word: &str) -> Vec<String> {
    search
        .search(word)
        .pages
        .into_iter()
        .flat_map(|page| page.sections)
        .map(|section| section.url)
        .collect()
}

/// A bus that, as each change is published, notes what the search index
/// finds for a word and whether a design resolves — what the listeners
/// before it and after it have done by then.
struct Probe {
    word: &'static str,
    design: &'static str,
    search: Mutex<Option<Arc<Search>>>,
    designs: Mutex<Option<Designs>>,
    seen: Mutex<Vec<(Change, Vec<String>, bool)>>,
}

impl Publish for Probe {
    fn publish(&self, change: Change) {
        let search = self.search.lock().unwrap().clone().unwrap();
        let designs = self.designs.lock().unwrap().clone().unwrap();
        let resolves = designs.resolve(self.design).is_ok();
        let found = found(&search, self.word);
        self.seen.lock().unwrap().push((change, found, resolves));
    }
}

#[test]
fn a_saved_page_refreshes_before_search_indexes_it() {
    let dir = served("order", "kind: site\ntitle: T\nroot: README.md\n");
    std::fs::write(
        dir.join("docs/README.md"),
        "# Home\n\n## Topics\n\n- [Guide](guide.md)\n",
    )
    .unwrap();
    let page = dir.join("docs/guide.md");
    std::fs::write(&page, "# Guide\n\nAn otter swims.\n").unwrap();
    let design = dir.join("docs/screen.design");
    std::fs::write(&design, "<Frame width={10} height={10} />\n").unwrap();

    let probe = Arc::new(Probe {
        word: "beaver",
        design: "docs/screen.design",
        search: Mutex::new(None),
        designs: Mutex::new(None),
        seen: Mutex::new(Vec::new()),
    });
    let listeners = listeners(&dir, probe.clone(), true);
    *probe.search.lock().unwrap() = listeners.search.clone();
    *probe.designs.lock().unwrap() = Some(listeners.designs.clone());
    let search = listeners.search.clone().unwrap();
    assert!(listeners.designs.resolve("docs/screen.design").is_ok());

    // One burst: the page saved with a new word, the design saved broken.
    std::fs::write(&page, "# Guide\n\nA beaver builds.\n").unwrap();
    std::fs::write(&design, "<Frame width={10} height={10} lable=\"x\" />\n").unwrap();
    listeners.changed(&[page, design]);

    // When open pages heard, the design store had forgotten the design and
    // the search index had not yet read the page.
    let seen = probe.seen.lock().unwrap().clone();
    assert_eq!(seen, [(Change::Refresh, Vec::new(), false)]);
    // And then it read it.
    assert_eq!(found(&search, "beaver"), ["/docs/guide.md#guide"]);
}

#[test]
fn a_changed_file_reaches_the_design_store_and_one_under_devpage_does_not() {
    let dir = served("designs", "kind: site\ntitle: T\nroot: README.md\n");
    std::fs::create_dir_all(dir.join(".devpage")).unwrap();
    let valid = "<Frame width={10} height={10} />\n";
    let broken = "<Frame width={10} height={10} lable=\"x\" />\n";
    std::fs::write(dir.join("docs/screen.design"), valid).unwrap();
    std::fs::write(dir.join(".devpage/screen.design"), valid).unwrap();
    let listeners = listeners(&dir, Arc::new(Recorder::default()), false);
    let designs = listeners.designs.clone();
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

#[test]
fn a_thread_is_heard_before_the_refresh_it_shares_a_burst_with() {
    let dir = served(
        "named",
        "kind: site\ntitle: T\nroot: README.md\nwatch: [\"**/*.md\"]\n",
    );
    let changes = Arc::new(Recorder::default());
    let listeners = listeners(&dir, changes.clone(), false);
    let thread = dir.join(".devpage/comments/docs/README.md/20261025T000000Z-abcd.yml");
    listeners.changed(&[thread, dir.join("docs/README.md")]);
    assert_eq!(
        changes.drain(),
        [Change::Comments("/docs/README.md".into()), Change::Refresh]
    );
}

#[test]
fn with_comments_off_a_thread_file_announces_nothing() {
    let dir = served("off", "kind: site\ntitle: T\nroot: README.md\n");
    let changes = Arc::new(Recorder::default());
    let folder = opened(&dir);
    let listeners = Listeners::new(
        folder.clone(),
        Designs::new(folder.served()),
        Modules::default(),
        None,
        changes.clone(),
        None,
    );
    let thread = dir.join(".devpage/comments/docs/README.md/20261025T000000Z-abcd.yml");
    listeners.changed(&[thread]);
    assert_eq!(changes.drain(), []);
}

#[test]
fn a_new_gitignore_rule_takes_effect_without_a_restart() {
    let dir = served("rule", "kind: site\ntitle: T\nroot: README.md\n");
    std::fs::create_dir_all(dir.join("docs/drafts")).unwrap();
    let listeners = listeners(&dir, Arc::new(Recorder::default()), false);
    let draft = dir.join("docs/drafts/idea.md");
    assert!(listeners.folder().matters(&draft, false));

    std::fs::write(dir.join("docs/.gitignore"), "build/\ndrafts/\n").unwrap();
    let reshaped = listeners.changed(&[dir.join("docs/.gitignore")]);
    assert!(reshaped, "the folders to watch may differ now");
    assert!(!listeners.folder().matters(&draft, false));
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
    let listeners = listeners(&dir, Arc::new(Recorder::default()), true);
    let search = listeners.search.clone().unwrap();
    assert_eq!(found(&search, "otter"), ["/docs/guide.md#guide"]);

    std::fs::write(&page, "# Guide\n\nA beaver builds.\n").unwrap();
    listeners.changed(std::slice::from_ref(&page));
    assert_eq!(found(&search, "beaver"), ["/docs/guide.md#guide"]);
    assert!(found(&search, "otter").is_empty());

    std::fs::remove_file(&page).unwrap();
    listeners.changed(&[page]);
    assert!(found(&search, "beaver").is_empty());
}
