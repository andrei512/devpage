//! The search indexes of published versions: each built on its version's
//! first search, frozen — committed once, no writer, no threads — and kept,
//! the most recently searched first, up to a budget of bytes for all of
//! them. Past it the least recently searched goes, and its next search
//! builds it again. A version has one index, whichever address it is
//! searched at: its results are moved to the address asked at.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use devpage_core::search::{Answer, Search};

/// 256 MB of indexes, for every version the server holds.
pub const BUDGET: u64 = 256 << 20;

/// A version's index: its document's id and its number.
pub type Key = (String, u64);

pub struct Searches {
    budget: u64,
    state: Mutex<State>,
    /// One lock per index being built, so searches arriving together build
    /// it once.
    building: Mutex<HashMap<Key, Arc<Mutex<()>>>>,
}

#[derive(Default)]
struct State {
    kept: HashMap<Key, Kept>,
    /// Bumped by every search: the larger, the more recent.
    clock: u64,
}

struct Kept {
    search: Arc<Search>,
    /// Where the version was served when its index was built: every address
    /// in it starts here.
    mount: Arc<str>,
    bytes: u64,
    searched: u64,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

impl Searches {
    pub fn new(budget: u64) -> Self {
        Searches {
            budget,
            state: Mutex::default(),
            building: Mutex::default(),
        }
    }

    /// `query` searched in `key`'s index, its results' addresses under
    /// `mount`, where the version is being searched. Blocks while the index
    /// builds.
    pub fn answer(
        &self,
        key: &Key,
        mount: &str,
        query: &str,
        build: impl FnOnce() -> Search,
    ) -> Answer {
        let (search, built) = self.get(key, mount, build);
        remount(search.search(query), &built, mount)
    }

    /// `key`'s index and where it was built: the one kept, or — on its first
    /// search, or after it was dropped — the one `build` makes, frozen,
    /// under `mount`.
    fn get(
        &self,
        key: &Key,
        mount: &str,
        build: impl FnOnce() -> Search,
    ) -> (Arc<Search>, Arc<str>) {
        if let Some(search) = self.kept(key) {
            return search;
        }
        let one = lock(&self.building).entry(key.clone()).or_default().clone();
        let _building = lock(&one);
        // Another search built it while this one waited.
        if let Some(search) = self.kept(key) {
            return search;
        }
        let search = Arc::new(build());
        let mount: Arc<str> = mount.into();
        let bytes = search.bytes();
        let mut state = lock(&self.state);
        state.clock += 1;
        let searched = state.clock;
        state.kept.insert(
            key.clone(),
            Kept {
                search: search.clone(),
                mount: mount.clone(),
                bytes,
                searched,
            },
        );
        self.drop_past_budget(&mut state, key);
        drop(state);
        lock(&self.building).remove(key);
        (search, mount)
    }

    /// The bytes of every index kept.
    #[cfg(test)]
    pub fn bytes(&self) -> u64 {
        lock(&self.state).kept.values().map(|kept| kept.bytes).sum()
    }

    /// How many indexes are kept.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        lock(&self.state).kept.len()
    }

    fn kept(&self, key: &Key) -> Option<(Arc<Search>, Arc<str>)> {
        let mut state = lock(&self.state);
        state.clock += 1;
        let now = state.clock;
        let kept = state.kept.get_mut(key)?;
        kept.searched = now;
        Some((kept.search.clone(), kept.mount.clone()))
    }

    /// Drops the least recently searched until the rest fit the budget,
    /// never `newest`: the index a search is about to use.
    fn drop_past_budget(&self, state: &mut State, newest: &Key) {
        while state.kept.values().map(|kept| kept.bytes).sum::<u64>() > self.budget {
            let Some(oldest) = state
                .kept
                .iter()
                .filter(|(key, _)| *key != newest)
                .min_by_key(|(_, kept)| kept.searched)
                .map(|(key, _)| key.clone())
            else {
                return;
            };
            state.kept.remove(&oldest);
        }
    }
}

/// `answer`, found in an index built under `from`, with its addresses under
/// `to` instead.
fn remount(mut answer: Answer, from: &str, to: &str) -> Answer {
    if from == to {
        return answer;
    }
    let moved = |address: &mut String| {
        if let Some(rest) = address.strip_prefix(from) {
            *address = format!("{to}{rest}");
        }
    };
    for page in &mut answer.pages {
        moved(&mut page.url);
        moved(&mut page.document.path);
        for section in &mut page.sections {
            moved(&mut section.url);
        }
    }
    answer
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use devpage_core::document::Document;
    use devpage_core::folder::{Folder, ServedFolder};

    use super::*;

    fn folder(name: &str, word: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("devpage-searches-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("devpage.yml"),
            "kind: site\ntitle: Site\nroot: README.md\n",
        )
        .unwrap();
        std::fs::write(dir.join("README.md"), format!("# Site\n\nA {word}.\n")).unwrap();
        dir.canonicalize().unwrap()
    }

    fn frozen(dir: &Path) -> Search {
        frozen_at(dir, LATEST)
    }

    /// The index of the version in `dir`, served under `mount`.
    fn frozen_at(dir: &Path, mount: &str) -> Search {
        let entry = dir.join("devpage.yml");
        let served = ServedFolder::new(dir).unwrap().mounted(mount);
        let document = Document::load(&entry, &served).unwrap();
        Search::frozen(&Folder::new(Arc::new(document)), &entry)
    }

    const LATEST: &str = "/d/doc";

    fn key(n: u64) -> Key {
        ("doc".into(), n)
    }

    #[test]
    fn a_versions_index_is_built_on_its_first_search_and_only_then() {
        let dir = folder("once", "walrus");
        let searches = Searches::new(BUDGET);
        let built = AtomicUsize::new(0);
        let build = || {
            built.fetch_add(1, Ordering::SeqCst);
            frozen(&dir)
        };
        assert_eq!(searches.len(), 0, "nothing built before a search");
        let (first, _) = searches.get(&key(1), LATEST, build);
        assert_eq!(first.search("walrus").pages.len(), 1);
        assert!(!first.is_live());
        let (again, _) = searches.get(&key(1), LATEST, || panic!("a second search builds nothing"));
        assert!(Arc::ptr_eq(&first, &again));
        assert_eq!(built.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn past_the_budget_the_least_recently_searched_goes_and_is_built_again() {
        let dirs: Vec<_> = (0..3)
            .map(|n| folder(&format!("budget-{n}"), &format!("word{n}")))
            .collect();
        let one = frozen(&dirs[0]).bytes();
        // Room for two indexes, not three.
        let searches = Searches::new(one * 2 + one / 2);
        searches.get(&key(0), LATEST, || frozen(&dirs[0]));
        searches.get(&key(1), LATEST, || frozen(&dirs[1]));
        // Version 0 searched again: version 1 is now the least recent.
        searches.get(&key(0), LATEST, || panic!("kept"));
        searches.get(&key(2), LATEST, || frozen(&dirs[2]));
        assert_eq!(searches.len(), 2);
        assert!(searches.bytes() <= one * 2 + one / 2);
        searches.get(&key(0), LATEST, || {
            panic!("version 0 was searched most recently")
        });
        let rebuilt = AtomicUsize::new(0);
        let (again, _) = searches.get(&key(1), LATEST, || {
            rebuilt.fetch_add(1, Ordering::SeqCst);
            frozen(&dirs[1])
        });
        assert_eq!(rebuilt.load(Ordering::SeqCst), 1, "dropped, so built again");
        assert_eq!(again.search("word1").pages.len(), 1, "and it still answers");
    }

    #[test]
    fn an_index_bigger_than_the_budget_still_answers_its_search() {
        let dir = folder("big", "heron");
        let searches = Searches::new(1);
        let (search, _) = searches.get(&key(1), LATEST, || frozen(&dir));
        assert_eq!(search.search("heron").pages.len(), 1);
        searches.get(&key(2), LATEST, || frozen(&dir));
        assert_eq!(searches.len(), 1, "only the newest is kept");
    }

    #[test]
    fn a_version_searched_at_both_its_addresses_has_one_index() {
        let dir = folder("mounts", "otter");
        let searches = Searches::new(BUDGET);
        let pinned = "/d/doc/v/1";
        let at_latest = searches.answer(&key(1), LATEST, "otter", || frozen_at(&dir, LATEST));
        let at_pinned = searches.answer(&key(1), pinned, "otter", || {
            panic!("the version's own address searches the index the latest built")
        });
        assert_eq!(searches.len(), 1);
        let (latest, pinned_page) = (&at_latest.pages[0], &at_pinned.pages[0]);
        assert!(latest.url.starts_with(LATEST), "{}", latest.url);
        assert!(!latest.url.starts_with(pinned), "{}", latest.url);
        assert!(pinned_page.url.starts_with(pinned), "{}", pinned_page.url);
        assert_eq!(
            pinned_page.url.strip_prefix(pinned),
            latest.url.strip_prefix(LATEST),
            "the same page, at each address"
        );
        assert!(pinned_page.document.path.starts_with(pinned));
    }
}
