//! What of the served folder is in sight, and what a change to it means for
//! open pages: the folders to watch (nothing out of sight is watched, so the
//! platform never reports a build rewriting `target/` — not even to be
//! dropped), whether a changed path can matter at all, judged from the path
//! alone, and whether it refreshes open pages, by the `watch` list of the
//! site that owns it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::Folder;
use crate::document::{Document, KindName};

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
    /// The watch set of `folder`, judged by its `.gitignore` rules and every
    /// site's `watch` list. Walks only what is in sight: an out-of-sight
    /// folder is never read.
    pub(super) fn plan(folder: &Folder) -> Self {
        let sight = Sight {
            served: folder.served().path(),
            folder,
            roots: watch_roots(folder),
        };
        let mut set = Self::default();
        if sight.walk(sight.served, &mut set) {
            set.deep.insert(sight.served.to_path_buf());
        }
        set
    }
}

/// What decides whether a folder is in sight.
struct Sight<'a> {
    served: &'a Path,
    folder: &'a Folder,
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

    /// Whether `folder` is watched: in sight by the `.gitignore` rules, or one of
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
        !self.folder.ignored(folder, true)
            || self.roots.iter().any(|root| {
                root.starts_with(folder)
                    || (folder.starts_with(root) && self.folder.ignored(root, true))
            })
    }
}

/// The literal prefixes of every site's `watch` patterns: the entry's, and
/// those of every indexed site.
fn watch_roots(folder: &Folder) -> Vec<PathBuf> {
    let mut roots = folder.entry().watch_roots().to_vec();
    for site in folder.sites() {
        roots.extend_from_slice(site.watch_roots());
    }
    roots
}

impl Folder {
    /// Whether a change to `path` can matter to anyone: not under `.git`,
    /// and either in sight or named by its site's `watch` list. A comment
    /// thread always matters; nothing else under `.devpage/` does. Decided
    /// from the path alone — no file system call — so a build rewriting an
    /// ignored folder costs a lookup per path. A path not under the served
    /// folder as given is kept, to be judged once canonical.
    pub fn matters(&self, path: &Path, is_dir: bool) -> bool {
        let Some(relative) = self.served().relative(path) else {
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
        !self.ignored(path, is_dir) || self.named_by_watch(path)
    }

    /// Whether a change to `path`, which is in sight, refreshes open pages.
    /// Document files and widget files always do; any other path is judged by
    /// the `watch` list of its owner, and refreshes when there is no such list.
    pub(super) fn refreshes(&self, path: &Path) -> bool {
        if self.kind(path).is_some() || self.is_widget_file(path) {
            return true;
        }
        match self.owning_site(path) {
            Some(Some(site)) => site.watches(path),
            // No site holds it, or its site's file is invalid.
            Some(None) | None => true,
        }
    }

    /// Whether the `watch` list of the site owning `path` names it.
    fn named_by_watch(&self, path: &Path) -> bool {
        self.owning_site(path)
            .flatten()
            .is_some_and(|site| site.names(path))
    }

    /// The site owning `path` as its `watch` list is judged: `None` when no
    /// site holds it, `Some(None)` when its site's file is invalid. The
    /// entry's site is the entry document as last read without error, so
    /// sifting a path it owns reads no file.
    fn owning_site(&self, path: &Path) -> Option<Option<Arc<Document>>> {
        let site = self.index.site_for(path)?;
        Some(if site == self.entry_file() {
            Some(self.entry())
        } else {
            Document::load(&site, self.served()).ok().map(Arc::new)
        })
    }

    /// Whether `path` is a widget or a helper in the widgets folder of some
    /// document.
    fn is_widget_file(&self, path: &Path) -> bool {
        path.parent().is_some_and(|folder| {
            crate::widgets::is_widget_file(path, folder) && self.widgets_owner(folder).is_some()
        })
    }

    /// Every indexed site but the entry, loaded; an invalid one is passed over.
    fn sites(&self) -> Vec<Document> {
        self.documents()
            .into_iter()
            .filter(|file| file != self.entry_file() && self.kind(file) == Some(KindName::Site))
            .filter_map(|file| Document::load(&file, self.served()).ok())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folder::ServedFolder;

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
        let served = ServedFolder::new(dir).unwrap();
        let document = Arc::new(Document::load(&dir.join("docs"), &served).unwrap());
        Folder::new(document).watched()
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
