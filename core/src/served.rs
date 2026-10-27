//! The served folder: the one place that decides whether a path is part of
//! what devpage serves. Every link, chapter, slide, widget folder, logo or URL
//! that names a file is resolved here — symbolic links followed — and anything
//! that lands outside the folder is refused. A leaf: it depends on nothing
//! else in devpage.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

static MOUNTS: LazyLock<Mutex<HashMap<PathBuf, String>>> = LazyLock::new(Mutex::default);

/// The canonical folder devpage serves from, and the boundary around it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedFolder(PathBuf, String);

impl ServedFolder {
    /// The served folder at `path`, made canonical; an error when it doesn't
    /// exist.
    pub fn new(path: &Path) -> std::io::Result<Self> {
        path.canonicalize().map(Self::canonical)
    }

    /// A served folder at a path the caller already made canonical — the
    /// index and the design store are handed the document's.
    pub fn canonical(path: PathBuf) -> Self {
        // PROTOTYPE: callers rebuild a served folder from a bare path in many
        // places, so the mount is looked up by path rather than carried.
        let mount = MOUNTS
            .lock()
            .unwrap()
            .get(&path)
            .cloned()
            .unwrap_or_default();
        Self(path, mount)
    }

    /// Serves the folder at canonical `path` under `mount` — `/d/<id>/v/<n>`
    /// in server mode — wherever a served folder is made for it.
    pub fn mount_at(path: &Path, mount: &str) {
        MOUNTS
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), mount.to_string());
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
    /// the mount prefix.
    pub fn url(&self, file: &Path) -> String {
        format!("{}/{}", self.1, self.name(file))
    }

    /// A file's name: its path from the top of the folder, as problems and
    /// labels show it.
    pub fn name(&self, file: &Path) -> String {
        self.relative(file).unwrap_or(file).display().to_string()
    }
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
        let top = std::env::temp_dir().join(format!("devpage-served-{name}"));
        let _ = std::fs::remove_dir_all(&top);
        std::fs::create_dir_all(top.join("served/docs")).unwrap();
        std::fs::write(top.join("outside.txt"), "secret").unwrap();
        std::fs::write(top.join("served/docs/a.md"), "# A").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(top.join("outside.txt"), top.join("served/link-out")).unwrap();
        let served = ServedFolder::new(&top.join("served")).unwrap();
        (top.canonicalize().unwrap(), served)
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
        assert_eq!(served.url(&a), "/docs/a.md");
        assert_eq!(served.relative(&a), Some(Path::new("docs/a.md")));
    }

    #[test]
    fn refuses_what_climbs_out_or_links_out() {
        let (top, served) = fixture("outside");
        assert_eq!(served.at("/../outside.txt"), None);
        assert_eq!(served.file_at("/../outside.txt"), None);
        assert_eq!(served.file(&top.join("served/../outside.txt")), None);
        #[cfg(unix)]
        {
            assert_eq!(served.at("/link-out"), None);
            assert_eq!(served.file(&top.join("served/link-out")), None);
        }
        assert_eq!(served.inside(&top.join("served/missing.md")), None);
        assert!(!served.contains(&top.join("outside.txt")));
        assert_eq!(served.relative(&top.join("outside.txt")), None);
    }

    #[test]
    fn a_deleted_file_is_still_inside_by_its_path() {
        let (top, served) = fixture("deleted");
        let gone = top.join("served/docs/gone.md");
        assert!(served.contains(&gone));
        assert_eq!(served.inside(&gone), None, "it no longer resolves");
    }
}
