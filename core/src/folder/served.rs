//! The served folder: the one place that decides whether a path is part of
//! what devpage serves. Every link, chapter, slide, widget folder, logo or URL
//! that names a file is resolved here — symbolic links followed — and anything
//! that lands outside the folder is refused. A leaf: it depends on nothing
//! else in devpage.

use std::path::{Path, PathBuf};

/// The canonical folder devpage serves from, the boundary around it, and the
/// prefix its addresses are under — its **mount**: empty for a local devpage
/// and bundled mode, `/d/<id>` or `/d/<id>/v/<n>` for a version in server
/// mode. It is made once, where devpage starts serving a folder, and passed
/// to everything loaded from it, so the mount reaches every address.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServedFolder(PathBuf, String);

impl ServedFolder {
    /// The served folder at `path`, made canonical; an error when it doesn't
    /// exist.
    pub fn new(path: &Path) -> std::io::Result<Self> {
        path.canonicalize().map(Self::canonical)
    }

    /// A served folder at a path the caller already made canonical, at `/`.
    pub fn canonical(path: PathBuf) -> Self {
        Self(path, String::new())
    }

    /// The same folder with its addresses under `mount`, which starts with
    /// `/` and doesn't end with one: `/d/<id>/v/<n>`.
    pub fn mounted(self, mount: &str) -> Self {
        debug_assert!(mount.is_empty() || (mount.starts_with('/') && !mount.ends_with('/')));
        Self(self.0, mount.to_string())
    }

    /// The prefix its addresses are under: empty at `/`.
    pub fn mount(&self) -> &str {
        &self.1
    }

    /// Its canonical path.
    pub fn path(&self) -> &Path {
        &self.0
    }

    /// `path` with symbolic links followed, when it exists inside the folder.
    pub fn inside(&self, path: &Path) -> Option<PathBuf> {
        let real = path.canonicalize().ok()?;
        real.starts_with(&self.0).then_some(real)
    }

    /// `path` resolved, when it is a file inside the folder.
    pub fn file(&self, path: &Path) -> Option<PathBuf> {
        self.inside(path).filter(|real| real.is_file())
    }

    /// `path` resolved, when it is a folder inside the folder.
    pub fn folder(&self, path: &Path) -> Option<PathBuf> {
        self.inside(path).filter(|real| real.is_dir())
    }

    /// The file `path` names, as the watcher reports it: resolved, when it
    /// is a file inside the folder; when nothing is there yet, the path it
    /// would have — its folder resolved on disk where that folder exists —
    /// so its arrival can be told apart. `None` for anything else: a path
    /// outside the folder, or a folder.
    pub fn named(&self, path: &Path) -> Option<PathBuf> {
        if let Some(file) = self.file(path) {
            return Some(file);
        }
        let lexical = lexical(path);
        let would_be = match (lexical.parent(), lexical.file_name()) {
            (Some(parent), Some(name)) => parent
                .canonicalize()
                .map(|parent| parent.join(name))
                .unwrap_or(lexical),
            _ => lexical,
        };
        (self.contains(&would_be) && std::fs::symlink_metadata(&would_be).is_err())
            .then_some(would_be)
    }

    /// What a URL path names — any entry, file or folder — when it lies
    /// inside the folder.
    pub fn at(&self, url_path: &str) -> Option<PathBuf> {
        self.inside(&self.0.join(url_path.trim_start_matches('/')))
    }

    /// The file a URL path names, when it is a file inside the folder.
    pub fn file_at(&self, url_path: &str) -> Option<PathBuf> {
        self.at(url_path).filter(|real| real.is_file())
    }

    /// Whether an already-resolved path lies inside the folder, without
    /// touching the disk: for paths that may no longer exist, such as a
    /// deleted file the watcher reports.
    pub fn contains(&self, path: &Path) -> bool {
        path.starts_with(&self.0)
    }

    /// A resolved path's place in the folder, `None` when it lies outside.
    pub fn relative<'a>(&self, path: &'a Path) -> Option<&'a Path> {
        path.strip_prefix(&self.0).ok()
    }

    /// A file's address: its path from the top of the folder, after `/` and
    /// the mount prefix. `None` when it lies outside: a path outside the
    /// folder has no address.
    pub fn url(&self, file: &Path) -> Option<String> {
        Some(format!("{}/{}", self.1, self.name(file)?))
    }

    /// A file's name: its path from the top of the folder, as problems and
    /// labels show it. `None` when it lies outside: a path outside the
    /// folder is never named.
    pub fn name(&self, file: &Path) -> Option<String> {
        Some(slashed(self.relative(file)?))
    }
}

/// A symbolic link at `link` to `target`, on every platform: on Windows a
/// file's link and a folder's are made differently. For tests that check
/// nothing is reached through a link out of the folder.
#[cfg(any(test, feature = "testing"))]
pub fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    return std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    return if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    };
}

/// A path inside the folder as addresses name it: its parts joined with `/`
/// on every platform, Windows too.
pub fn slashed(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// `path` with its `.` and `..` components resolved by name, not on disk:
/// the path a link or an import names when the file may not exist yet, so it
/// still gets an address (and a problem). The one rule for it.
pub(crate) fn lexical(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

impl AsRef<Path> for ServedFolder {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A served folder `served/` beside a secret `outside.txt`, with a
    /// symbolic link inside pointing out.
    fn fixture(name: &str) -> (PathBuf, ServedFolder) {
        let top =
            std::env::temp_dir().join(format!("devpage-served-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&top);
        std::fs::create_dir_all(top.join("served/docs")).unwrap();
        std::fs::write(top.join("outside.txt"), "secret").unwrap();
        std::fs::write(top.join("served/docs/a.md"), "# A").unwrap();
        symlink(&top.join("outside.txt"), &top.join("served/link-out")).unwrap();
        let served = ServedFolder::new(&top.join("served")).unwrap();
        (top.canonicalize().unwrap(), served)
    }

    /// A nested file's name and address join its parts with `/` on every
    /// platform: on Windows the path's own separator is `\`.
    #[test]
    fn names_and_addresses_join_with_slashes() {
        let (top, served) = fixture("slashes");
        let a = served
            .file(&top.join("served").join("docs").join("a.md"))
            .unwrap();
        assert_eq!(served.name(&a).as_deref(), Some("docs/a.md"));
        assert_eq!(served.url(&a).as_deref(), Some("/docs/a.md"));
        std::fs::remove_dir_all(&top).ok();
    }

    #[test]
    fn resolves_files_and_folders_inside() {
        let (top, served) = fixture("inside");
        let a = top.join("served/docs/a.md");
        assert_eq!(
            served.file(&top.join("served/docs/../docs/a.md")),
            Some(a.clone())
        );
        assert_eq!(
            served.folder(&top.join("served/docs")),
            Some(top.join("served/docs"))
        );
        assert_eq!(
            served.file(&top.join("served/docs")),
            None,
            "a folder is no file"
        );
        assert_eq!(served.folder(&a), None, "a file is no folder");
        assert_eq!(served.at("/docs/a.md"), Some(a.clone()));
        assert_eq!(served.file_at("/docs/a.md"), Some(a.clone()));
        assert_eq!(served.file_at("/docs"), None, "a folder is no file");
        assert_eq!(served.url(&a).as_deref(), Some("/docs/a.md"));
        assert_eq!(served.name(&a).as_deref(), Some("docs/a.md"));
        assert_eq!(served.relative(&a), Some(Path::new("docs/a.md")));
    }

    #[test]
    fn a_mounted_folder_addresses_its_files_under_the_mount() {
        let (top, served) = fixture("mounted");
        let a = top.join("served/docs/a.md");
        let mounted = served.clone().mounted("/d/x/v/2");
        assert_eq!(mounted.url(&a).as_deref(), Some("/d/x/v/2/docs/a.md"));
        assert_eq!(
            mounted.name(&a).as_deref(),
            Some("docs/a.md"),
            "a name is the same anywhere"
        );
        assert_eq!(mounted.mount(), "/d/x/v/2");
        // A folder made from its path is served at `/`.
        assert_eq!(ServedFolder::new(&top.join("served")).unwrap(), served);
    }

    #[test]
    fn refuses_what_climbs_out_or_links_out() {
        let (top, served) = fixture("outside");
        assert_eq!(served.at("/../outside.txt"), None);
        assert_eq!(served.file_at("/../outside.txt"), None);
        assert_eq!(served.file(&top.join("served/../outside.txt")), None);
        assert_eq!(served.at("/link-out"), None);
        assert_eq!(served.file(&top.join("served/link-out")), None);
        assert_eq!(served.inside(&top.join("served/missing.md")), None);
        assert!(!served.contains(&top.join("outside.txt")));
        assert_eq!(served.relative(&top.join("outside.txt")), None);
        // Neither named nor addressed: no absolute path stands in.
        assert_eq!(served.name(&top.join("outside.txt")), None);
        assert_eq!(served.url(&top.join("outside.txt")), None);
        assert_eq!(
            served.clone().mounted("/d/x").url(&top.join("outside.txt")),
            None
        );
    }

    #[test]
    fn a_deleted_file_is_still_inside_by_its_path() {
        let (top, served) = fixture("deleted");
        let gone = top.join("served/docs/gone.md");
        assert!(served.contains(&gone));
        assert_eq!(served.inside(&gone), None, "it no longer resolves");
    }
}
