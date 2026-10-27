//! The watch set: which folders of the served folder are watched, and how.
//! Nothing out of sight is watched, so the platform never reports a build
//! rewriting `target/` to devpage — not even to be dropped.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::document::{Document, Entry, KindName};
use crate::index::Index;

/// The folders to watch: `deep` ones recursively, `shallow` ones for their
/// entries and the files directly in them.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WatchSet {
    /// The largest folders with nothing out of sight anywhere under them.
    pub deep: BTreeSet<PathBuf>,
    /// Folders in sight that hold something out of sight.
    pub shallow: BTreeSet<PathBuf>,
}

impl WatchSet {
    /// The watch set of the canonical served folder `served`, judged by the
    /// index's rules and every site's `watch` list. Walks only what is in
    /// sight: an out-of-sight folder is never read.
    pub fn plan(served: &Path, index: &Index, entry: &Entry) -> Self {
        let sight = Sight {
            served,
            index,
            roots: watch_roots(served, index, entry),
        };
        let mut set = Self::default();
        if sight.walk(served, &mut set) {
            set.deep.insert(served.to_path_buf());
        }
        set
    }
}

/// What decides whether a folder is in sight.
struct Sight<'a> {
    served: &'a Path,
    index: &'a Index,
    /// Where any site's `watch` list can name a file.
    roots: Vec<PathBuf>,
}

impl Sight<'_> {
    /// Records the watched folders under `folder`, which is in sight, and
    /// says whether nothing out of sight is anywhere under it — so the
    /// caller can watch it whole instead.
    fn walk(&self, folder: &Path, set: &mut WatchSet) -> bool {
        let Ok(entries) = std::fs::read_dir(folder) else {
            return true;
        };
        // `.devpage` is out of sight itself; only its comment store is watched whole.
        let mut clean = folder != self.served.join(".devpage");
        let mut whole = Vec::new();
        for entry in entries.flatten() {
            // A symbolic link is not followed, by this walk or by the platform.
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let child = entry.path();
            if !self.in_sight(&child) {
                clean = false;
            } else if self.walk(&child, set) {
                whole.push(child);
            } else {
                clean = false;
            }
        }
        if !clean {
            set.shallow.insert(folder.to_path_buf());
            set.deep.extend(whole);
        }
        clean
    }

    /// Whether `folder` is watched: in sight by the index's rules, or one of
    /// the two exceptions — `.devpage` and its comment store, and an ignored
    /// folder a `watch` pattern reaches into: its literal prefix is in the
    /// folder or is it, or is an ignored folder holding it. A prefix above
    /// every ignored folder — `**/*.md` — reaches into none.
    fn in_sight(&self, folder: &Path) -> bool {
        let Ok(relative) = folder.strip_prefix(self.served) else {
            return false;
        };
        if relative == Path::new(".devpage") || relative.starts_with(".devpage/comments") {
            return true;
        }
        if relative.starts_with(".devpage")
            || relative.components().any(|part| part.as_os_str() == ".git")
        {
            return false;
        }
        !self.index.ignored(folder, true)
            || self.roots.iter().any(|root| {
                root.starts_with(folder)
                    || (folder.starts_with(root) && self.index.ignored(root, true))
            })
    }
}

/// The literal prefixes of every site's `watch` patterns: the entry's, and
/// those of every indexed site.
fn watch_roots(served: &Path, index: &Index, entry: &Entry) -> Vec<PathBuf> {
    let document = entry.document();
    let mut roots = document.watch_roots().to_vec();
    for file in index.documents() {
        if file != entry.file()
            && index.kind(&file) == Some(KindName::Site)
            && let Ok(site) = Document::load(&file, served)
        {
            roots.extend_from_slice(site.watch_roots());
        }
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A served folder holding `files` (empty ones, folders made as needed)
    /// and a site in `docs/` with `site` as its document file.
    fn served(name: &str, site: &str, files: &[&str]) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("devpage-watch-set-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(dir.join("docs/README.md"), "# Home").unwrap();
        std::fs::write(dir.join("docs/devpage.yml"), site).unwrap();
        for file in files {
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            if file.ends_with('/') {
                std::fs::create_dir_all(&path).unwrap();
            } else {
                std::fs::write(&path, "").unwrap();
            }
        }
        dir.canonicalize().unwrap()
    }

    fn plan(dir: &Path) -> WatchSet {
        let document = Arc::new(Document::load(&dir.join("docs"), dir).unwrap());
        WatchSet::plan(dir, &Index::build(dir), &Entry::new(document))
    }

    fn set(dir: &Path, deep: &[&str], shallow: &[&str]) -> WatchSet {
        let join = |folders: &[&str]| folders.iter().map(|f| dir.join(f)).collect();
        WatchSet {
            deep: join(deep),
            shallow: join(shallow),
        }
    }

    const SITE: &str = "kind: site\ntitle: T\nroot: README.md\n";

    #[test]
    fn a_folder_with_nothing_out_of_sight_is_watched_whole() {
        let dir = served("whole", SITE, &["src/main.rs", "docs/guides/a.md"]);
        assert_eq!(plan(&dir), set(&dir, &[""], &[]));
    }

    #[test]
    fn ignored_folders_and_git_leave_their_parents_shallow() {
        let dir = served(
            "repo",
            SITE,
            &[
                ".gitignore",
                ".git/HEAD",
                "target/debug/devpage",
                "src/main.rs",
                "crate/.gitignore",
                "crate/src/lib.rs",
                "crate/target/debug/x",
                "crate/benches/b.rs",
            ],
        );
        std::fs::write(dir.join(".gitignore"), "/target/\n").unwrap();
        std::fs::write(dir.join("crate/.gitignore"), "target/\n").unwrap();
        assert_eq!(
            plan(&dir),
            set(
                &dir,
                &["docs", "src", "crate/src", "crate/benches"],
                &["", "crate"]
            )
        );
    }

    #[test]
    fn the_comment_store_is_watched_and_nothing_else_in_devpage() {
        let dir = served(
            "devpage",
            SITE,
            &[".devpage/comments/docs/README.md/x.yml", ".devpage/cache/y"],
        );
        assert_eq!(
            plan(&dir),
            set(&dir, &["docs", ".devpage/comments"], &["", ".devpage"])
        );
    }

    #[test]
    fn an_ignored_folder_a_watch_pattern_reaches_into_is_watched() {
        let dir = served(
            "named",
            "kind: site\ntitle: T\nroot: README.md\nwatch: [\"build/*.svg\"]\n",
            &[
                "docs/.gitignore",
                "docs/build/chart.svg",
                "docs/build/deep/x",
                "docs/out/x",
                ".gitignore",
                "target/x",
            ],
        );
        std::fs::write(dir.join("docs/.gitignore"), "build/\nout/\n").unwrap();
        std::fs::write(dir.join(".gitignore"), "/target/\n").unwrap();
        // `build/` is named; `out/` and `target/` are not.
        assert_eq!(plan(&dir), set(&dir, &["docs/build"], &["", "docs"]));
    }

    #[test]
    fn a_pattern_that_only_matches_into_an_ignored_folder_does_not_watch_it() {
        let dir = served(
            "glob",
            "kind: site\ntitle: T\nroot: README.md\nwatch: [\"**/*.md\"]\n",
            &[
                "docs/.gitignore",
                "docs/build/out.md",
                "docs/guides/a.md",
                ".gitignore",
                "target/x",
            ],
        );
        std::fs::write(dir.join("docs/.gitignore"), "build/\n").unwrap();
        std::fs::write(dir.join(".gitignore"), "/target/\n").unwrap();
        // `**/*.md` can match in `docs/build/`, but reaches into no ignored folder.
        assert_eq!(plan(&dir), set(&dir, &["docs/guides"], &["", "docs"]));
    }

    #[test]
    fn a_pattern_reaching_into_an_ignored_folder_watches_the_folders_under_it() {
        let dir = served(
            "under",
            "kind: site\ntitle: T\nroot: README.md\nwatch: [\"build/**/*.svg\"]\n",
            &["docs/.gitignore", "docs/build/deep/chart.svg"],
        );
        std::fs::write(dir.join("docs/.gitignore"), "build/\n").unwrap();
        assert_eq!(plan(&dir), set(&dir, &[""], &[]));
    }
}
