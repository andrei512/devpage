//! The design store: every design asked for kept resolved in memory and
//! resolved again only when a save touched what it read, its problems and
//! its PNG. The watch stream and the canvas page that show designs in the
//! browser are the server's, in `server::watch_stream` and `server::canvas`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use devpage_design::bands::Piece;
use devpage_design::fonts::FontFile;
use devpage_design::proto;
use devpage_design::render::{Drawing, Undrawable};
use devpage_design::resolve::ImageFile;
use devpage_design::resolve::{Outcome, Resolution};
use devpage_design::tiles::{Cache, Key, Level, TILE, Tiled};
use devpage_design::wire::Applied;
use devpage_design::{Files, Problem, Renderer, Scene, Theme};

use crate::served::ServedFolder;

/// What the store knows of a design without locking it: which version of
/// its scene it shows or its problems, every file resolving it asked for —
/// missing ones included, so creating one counts — and whether its own file
/// is missing.
#[derive(Clone, PartialEq)]
struct Resolved {
    version: u64,
    problems: Option<Vec<Problem>>,
    asked: Vec<String>,
    /// The design's own file doesn't exist.
    missing: bool,
}

/// A file as last read: its bytes, or `None` when it doesn't exist or lies
/// outside the served folder. A folder's listing is kept the same way, under
/// its path and a `/`, as the names of its files, one per line.
struct Source {
    bytes: Option<Vec<u8>>,
    /// Which read of the store's this was, counting from one: a result made
    /// from it is current only while the file is still this read.
    read: u64,
    /// Its canonical path from the top of the served folder, which is what the
    /// watcher reports.
    canonical: Option<String>,
    /// The file decoded as an image, once a design shows it: forgotten with
    /// the file, so a save decodes only the image that changed.
    image: Option<Result<Arc<ImageFile>, String>>,
    /// The file checked as a font, once a design's text is set in it:
    /// forgotten with the file, like `image`.
    font: Option<Result<Arc<FontFile>, String>>,
}

/// A design as the store keeps it: kept resolved — its last valid scene, and
/// where each node came from — with that scene's version, its problems now,
/// its drawing in each theme it shows in, and the patches that brought the
/// scene to its version, for the watch streams.
#[derive(Default)]
struct Kept {
    /// What tells this design's tiles apart in the store's cache.
    id: u64,
    /// The scene's version: one more for each patch that changed it.
    version: u64,
    resolution: Option<Resolution>,
    /// The design's problems now — resolving it, or drawing its scene.
    problems: Option<Vec<Problem>>,
    drawings: Vec<Drawing>,
    /// The last patches, each from the version before it: what a stream that
    /// sent that version sends next.
    log: std::collections::VecDeque<(u64, Arc<proto::Patch>)>,
    asked: Vec<String>,
    missing: bool,
}

/// How many patches a design keeps for streams behind: one further behind
/// is sent the whole scene.
const LOG: usize = 64;

/// What a watch stream sends next for a design, after the version it sent.
pub enum Next {
    /// The design has these problems.
    Problems(Vec<Problem>),
    /// Its whole scene, at this version: the stream sent none, or is too far
    /// behind for patches.
    Scene(u64, Box<proto::Scene>),
    /// The patches from the version sent to this one, as one.
    Patch(u64, proto::Patch),
    /// The stream is current; the scene is themed or not.
    Current(bool),
}

/// A tile in the store's cache: which kept design it is of, and which tile.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Tile {
    design: u64,
    key: Key,
}

impl Tiled for Tile {
    fn key(&self) -> Key {
        self.key
    }
}

/// What the store's tiles may hold together: 64 MB of pixels.
pub const TILES_CAP: usize = 64 << 20;

/// What a tile's pixels cost in the cache.
const TILE_BYTES: usize = (TILE * TILE * 4) as usize;

/// What the store tells when a design's scene or problems change.
type Listener = Box<dyn Fn(&str) + Send + Sync>;

struct Store {
    served: ServedFolder,
    renderer: Mutex<Renderer>,
    sources: Mutex<HashMap<String, Source>>,
    /// How many files the store has read from disk.
    reads: std::sync::atomic::AtomicU64,
    /// Locked before `sources` whenever both are.
    resolved: Mutex<HashMap<String, Resolved>>,
    /// What tells this store's versions from another run's, in an ETag.
    nonce: u64,
    /// Told the address of each design whose scene or problems changed.
    listeners: Mutex<Vec<Listener>>,
    /// How many resolves the store has started, numbering each.
    versions: std::sync::atomic::AtomicU64,
    /// Each design as it is kept, locked one design at a time — while it is
    /// resolved again, and before `renderer`, `resolved` and `tiles`
    /// whenever they are.
    kept: Mutex<HashMap<String, Arc<Mutex<Kept>>>>,
    /// Every design's tiles at 1×, in each theme, shared and capped.
    tiles: Mutex<Cache<Arc<[u8]>, Tile>>,
    /// Each design file's last parse, by path, kept across its saves: a new
    /// parse is matched against it, so its elements keep their ids.
    syntax: Mutex<HashMap<String, Arc<devpage_design::parse::File>>>,
    /// How many tiles the store has painted, for the tests.
    #[cfg(test)]
    painted: std::sync::atomic::AtomicUsize,
    /// How many times a design was resolved, for the tests.
    #[cfg(test)]
    resolutions: std::sync::atomic::AtomicUsize,
    /// Called after each file read from disk, for the tests: a save can land
    /// there.
    #[cfg(test)]
    after_read: Mutex<Option<AfterRead>>,
}

#[cfg(test)]
type AfterRead = Box<dyn Fn(&str) + Send + Sync>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The store's files, recording every path a resolving asks for and which
/// read of it the resolving saw.
struct Reading<'a> {
    store: &'a Store,
    asked: RefCell<Vec<String>>,
    seen: RefCell<Vec<(String, u64)>>,
}

impl Reading<'_> {
    /// What `key` holds — a file's bytes, or a folder's listing when it ends
    /// in `/` — read once and kept until the watcher reports it changed.
    fn source(&self, key: &str) -> Option<Vec<u8>> {
        self.asked.borrow_mut().push(key.to_string());
        let mut sources = lock(&self.store.sources);
        if let Some(source) = sources.get(key) {
            self.seen.borrow_mut().push((key.to_string(), source.read));
            return source.bytes.clone();
        }
        let served = &self.store.served;
        let source = match key.strip_suffix('/') {
            Some(folder) => {
                let found = served.folder(&served.path().join(folder));
                Source {
                    bytes: found
                        .as_ref()
                        .and_then(|f| names(f))
                        .map(|names| names.join("\n").into_bytes()),
                    canonical: found
                        .as_ref()
                        .map(|f| format!("{}/", self.store.relative(f))),
                    read: self.store.next_read(),
                    image: None,
                    font: None,
                }
            }
            None => {
                let found = served.file(&served.path().join(key));
                Source {
                    bytes: found.as_ref().and_then(|f| std::fs::read(f).ok()),
                    canonical: found.as_ref().map(|f| self.store.relative(f)),
                    read: self.store.next_read(),
                    image: None,
                    font: None,
                }
            }
        };
        self.seen.borrow_mut().push((key.to_string(), source.read));
        let bytes = source.bytes.clone();
        sources.insert(key.to_string(), source);
        drop(sources);
        #[cfg(test)]
        if let Some(after) = lock(&self.store.after_read).as_ref() {
            after(key);
        }
        bytes
    }
}

/// The names of the files directly in `folder`, sorted.
fn names(folder: &Path) -> Option<Vec<String>> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    names.sort();
    Some(names)
}

impl Files for Reading<'_> {
    /// Any file's bytes — a design's text, an image's or a font's — read
    /// once and kept until the watcher reports it changed.
    fn bytes(&self, path: &str) -> Option<Vec<u8>> {
        self.source(path)
    }

    /// A design file parsed and matched against its last parse, so its
    /// elements keep their ids across saves.
    fn parse(&self, path: &str, source: &str) -> Result<Arc<devpage_design::parse::File>, Problem> {
        let fresh = devpage_design::parse::parse(source)?;
        let mut syntax = lock(&self.store.syntax);
        let file = Arc::new(match syntax.get(path) {
            Some(last) => devpage_design::ids::matched(fresh, last),
            None => fresh,
        });
        syntax.insert(path.to_string(), file.clone());
        Ok(file)
    }

    /// A folder's files, listed once and kept until the watcher reports a
    /// change to any file directly in it.
    fn list(&self, path: &str) -> Option<Vec<String>> {
        let listing = self.source(&format!("{path}/"))?;
        let text = String::from_utf8(listing).ok()?;
        Some(text.lines().map(str::to_string).collect())
    }

    /// A font file checked, once per read of it.
    fn font(&self, path: &str) -> Option<Result<Arc<FontFile>, String>> {
        if let Some(source) = lock(&self.store.sources).get(path)
            && let Some(font) = &source.font
        {
            self.asked.borrow_mut().push(path.to_string());
            self.seen.borrow_mut().push((path.to_string(), source.read));
            return Some(font.clone());
        }
        let bytes = self.bytes(path)?;
        let read = self.seen.borrow().last().map(|(_, read)| *read);
        let font = FontFile::new(bytes).map(Arc::new);
        if let Some(source) = lock(&self.store.sources).get_mut(path)
            && Some(source.read) == read
        {
            source.font = Some(font.clone());
        }
        Some(font)
    }

    /// An image file decoded, once per read of it.
    fn image(&self, path: &str) -> Option<Result<Arc<ImageFile>, String>> {
        if let Some(source) = lock(&self.store.sources).get(path)
            && let Some(image) = &source.image
        {
            self.asked.borrow_mut().push(path.to_string());
            self.seen.borrow_mut().push((path.to_string(), source.read));
            return Some(image.clone());
        }
        let bytes = self.bytes(path)?;
        let read = self.seen.borrow().last().map(|(_, read)| *read);
        let image = ImageFile::new(bytes).map(Arc::new);
        if let Some(source) = lock(&self.store.sources).get_mut(path)
            && Some(source.read) == read
        {
            source.image = Some(image.clone());
        }
        Some(image)
    }
}

impl Store {
    /// The number of the next read from disk, counting from one.
    fn next_read(&self) -> u64 {
        1 + self
            .reads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    fn relative(&self, file: &Path) -> String {
        self.served
            .relative(file)
            .unwrap_or(file)
            .to_string_lossy()
            .into_owned()
    }

    /// The design at `path` brought up to the files as they are now: the
    /// first time resolved whole, then resolved again only as far as the
    /// files in `changed` reach — and kept. A file the resolving read may
    /// change while it runs, after the read and before the result is kept,
    /// and `changed` would then find nothing kept to resolve again; so every
    /// file it read is checked to still be that read, and what isn't is
    /// resolved again.
    fn refresh(&self, path: &str, changed: &[String]) -> Resolved {
        let kept = self.kept(path);
        let mut k = lock(&kept);
        let mut changed = changed.to_vec();
        loop {
            let seen = self.bring(path, &mut k, &changed);
            let sources = lock(&self.sources);
            changed = seen
                .into_iter()
                .filter(|(file, read)| sources.get(file).is_none_or(|s| s.read != *read))
                .map(|(file, _)| file)
                .collect();
            drop(sources);
            if changed.is_empty() {
                break;
            }
        }
        let resolved = Resolved {
            version: k.version,
            problems: k.problems.clone(),
            asked: k.asked.clone(),
            missing: k.missing,
        };
        lock(&self.resolved).insert(path.to_string(), resolved.clone());
        resolved
    }

    /// Resolves the design at `path` — whole, or again as far as `changed`
    /// reaches — and brings its drawings across; says which read of each
    /// file it saw.
    fn bring(&self, path: &str, k: &mut Kept, changed: &[String]) -> Vec<(String, u64)> {
        #[cfg(test)]
        self.resolutions
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let reading = Reading {
            store: self,
            asked: RefCell::new(Vec::new()),
            seen: RefCell::new(Vec::new()),
        };
        enum Done {
            Fresh(Box<Resolution>),
            Again(Outcome),
        }
        let done = guarded(
            path,
            || match k.resolution.as_mut() {
                None => Resolution::new(&reading, path).map(|r| Done::Fresh(Box::new(r))),
                Some(res) => Ok(Done::Again(res.update(&reading, changed))),
            },
            |p| vec![p],
        );
        let asked = reading.asked.into_inner();
        k.missing = lock(&self.sources)
            .get(path)
            .is_none_or(|source| source.bytes.is_none());
        match done {
            Ok(Done::Fresh(res)) => {
                if k.id == 0 {
                    k.id = 1 + self
                        .versions
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                k.resolution = Some(*res);
                k.version += 1;
                // No stream holds this scene yet, or holds one it can't be
                // patched into: each is sent it whole.
                k.log.clear();
                k.asked = asked;
                k.problems = self.draw(path, k, None).err();
            }
            Ok(Done::Again(Outcome::Same)) => {
                k.asked.extend(asked);
                let shown = k.resolution.as_ref().map_or(0, |r| themes(r.scene()).len());
                k.problems = if k.drawings.len() == shown {
                    None
                } else {
                    self.draw(path, k, None).err()
                };
            }
            Ok(Done::Again(Outcome::Changed(patch, applied))) => {
                k.asked.extend(asked);
                k.log.push_back((k.version, Arc::new(patch)));
                if k.log.len() > LOG {
                    k.log.pop_front();
                }
                k.version += 1;
                k.problems = self.draw(path, k, Some(&applied)).err();
            }
            Ok(Done::Again(Outcome::Problems(problems))) => {
                k.asked.extend(asked);
                k.problems = Some(problems);
            }
            Err(problems) => {
                // Resolving failed — or the engine did, and what it kept may
                // be half done: the next change resolves the design afresh.
                k.asked.extend(asked);
                if problems
                    .iter()
                    .any(|p| p.message.starts_with("the design engine failed"))
                {
                    k.resolution = None;
                }
                k.problems = Some(problems);
            }
        }
        k.asked.sort();
        k.asked.dedup();
        reading.seen.into_inner()
    }

    /// The kept state of the design at `path`, made the first time.
    fn kept(&self, path: &str) -> Arc<Mutex<Kept>> {
        lock(&self.kept)
            .entry(path.to_string())
            .or_default()
            .clone()
    }

    /// Brings the design's drawings to its scene: each theme's drawing
    /// updated by `applied` — only the tiles over the damage going stale —
    /// or made, the first time or with no patch; a theme the design no
    /// longer shows in dropped, with its tiles. Or why the scene can't be
    /// drawn, its drawings the scene's all the same, so the next scene
    /// updates from it.
    fn draw(
        &self,
        path: &str,
        k: &mut Kept,
        applied: Option<&Applied>,
    ) -> Result<(), Vec<Problem>> {
        let Some(res) = k.resolution.as_ref() else {
            return Ok(());
        };
        let scene = res.scene();
        let themes = themes(scene);
        let id = k.id;
        let mut renderer = lock(&self.renderer);
        let mut broke: Option<Undrawable> = None;
        let mut drawings = Vec::new();
        for &theme in themes {
            let at = applied.and(k.drawings.iter().position(|d| d.theme() == theme));
            let made = guarded(
                path,
                || match at {
                    Some(i) => {
                        let mut d = k.drawings.swap_remove(i);
                        let done = d.update(
                            &mut renderer,
                            scene,
                            applied.expect("drawings with a patch"),
                        );
                        let mut tiles = lock(&self.tiles);
                        let mine = |t: &Tile| t.design == id && t.key.theme == theme;
                        match &done {
                            Ok(damage) if damage.all => tiles.damage_all_where(mine),
                            Ok(damage) => {
                                let a = d.area();
                                tiles.damage_where(mine, (a.x as f64, a.y as f64), &damage.rects)
                            }
                            // Nothing is painted from a drawing that can't be.
                            Err(_) => tiles.forget(mine),
                        }
                        drop(tiles);
                        Ok((Some(d), done.err()))
                    }
                    None => {
                        // A drawing made afresh owes nothing to tiles
                        // painted before it.
                        lock(&self.tiles).forget(|t: &Tile| t.design == id && t.key.theme == theme);
                        match renderer.drawing(scene, theme, None) {
                            Ok(d) => Ok((Some(d), None)),
                            Err(u) => Ok((None, Some(u))),
                        }
                    }
                },
                |p| Undrawable {
                    message: p.message,
                    at: None,
                },
            );
            let (drawing, problem) = match made {
                Ok(made) => made,
                // A drawing the engine panicked on is made again next time.
                Err(u) => (None, Some(u)),
            };
            drawings.extend(drawing);
            if broke.is_none() {
                broke = problem;
            }
        }
        drop(renderer);
        // The themes the design no longer shows in, and their tiles.
        lock(&self.tiles).forget(|t| t.design == id && !themes.contains(&t.key.theme));
        k.drawings = drawings;
        match broke {
            None => Ok(()),
            Some(u) => Err(problem(path, u)),
        }
    }

    /// The design at `path`, from memory, resolving it the first time.
    fn get(&self, path: &str) -> Resolved {
        if let Some(resolved) = lock(&self.resolved).get(path) {
            return resolved.clone();
        }
        self.refresh(path, &[])
    }
}

/// Every design the server has been asked for, kept resolved in memory, and
/// the renderer that paints them: its fonts load once.
#[derive(Clone)]
pub struct Designs(Arc<Store>);

/// A design's address as the engine names files: from the top of the served
/// folder, without a leading slash.
pub fn address(path: &str) -> &str {
    path.trim_start_matches('/')
}

impl Designs {
    /// The store for the served folder.
    pub fn new(served: &ServedFolder) -> Designs {
        Designs(Arc::new(Store {
            served: served.clone(),
            renderer: Mutex::new(Renderer::new()),
            sources: Mutex::default(),
            reads: Default::default(),
            resolved: Mutex::default(),
            nonce: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos() as u64),
            listeners: Mutex::default(),
            versions: Default::default(),
            kept: Mutex::default(),
            tiles: Mutex::new(Cache::<Arc<[u8]>, Tile>::new(TILES_CAP)),
            syntax: Mutex::default(),
            #[cfg(test)]
            painted: Default::default(),
            #[cfg(test)]
            resolutions: Default::default(),
            #[cfg(test)]
            after_read: Mutex::default(),
        }))
    }

    /// The design at `path` resolved — its scene, copied — or its problems.
    pub fn resolve(&self, path: &str) -> Result<Scene, Vec<Problem>> {
        let path = address(path);
        let resolved = self.0.get(path);
        if let Some(problems) = resolved.problems {
            return Err(problems);
        }
        let kept = self.0.kept(path);
        let k = lock(&kept);
        Ok(k.resolution
            .as_ref()
            .expect("a design with no problems is resolved")
            .scene()
            .clone())
    }

    /// What a watch stream that last sent the design at `path` at version
    /// `sent` — or nothing, `None` — sends next: the design's problems, its
    /// whole scene, the patches since that version as one, or nothing new.
    /// The patches were made once, when the design was resolved again, and
    /// every stream sends the same.
    pub fn next(&self, path: &str, sent: Option<u64>) -> Next {
        let path = address(path);
        let resolved = self.0.get(path);
        if let Some(problems) = resolved.problems {
            return Next::Problems(problems);
        }
        let kept = self.0.kept(path);
        let k = lock(&kept);
        let Some(res) = k.resolution.as_ref() else {
            return Next::Problems(Vec::new());
        };
        let scene = res.scene();
        let whole = || Next::Scene(k.version, Box::new(devpage_design::wire::scene(scene)));
        match sent {
            None => whole(),
            Some(v) if v == k.version => Next::Current(scene.themed),
            Some(v) => {
                let Some(first) = k.log.iter().position(|(from, _)| *from == v) else {
                    return whole();
                };
                let mut patch = proto::Patch {
                    themed: scene.themed,
                    ..Default::default()
                };
                for (_, p) in k.log.iter().skip(first) {
                    patch.ops.extend(p.ops.iter().cloned());
                    patch.images.extend(p.images.iter().cloned());
                    patch.fonts.extend(p.fonts.iter().cloned());
                }
                Next::Patch(k.version, patch)
            }
        }
    }

    /// Tells `listener` the address of each design whose scene or problems
    /// change from now on, as the engine names it — what
    /// [`Designs::resolve`] then answers anew. It is called on the thread
    /// that reported the change, so it should only pass the address on.
    pub fn on_change(&self, listener: impl Fn(&str) + Send + Sync + 'static) {
        lock(&self.0.listeners).push(Box::new(listener));
    }

    /// A file in the served folder changed, was created or was deleted: forget
    /// it and its folder's listing, resolve again every design that asked for
    /// either, and announce the designs whose scene or problems changed.
    pub fn changed(&self, file: &Path) {
        let store = &self.0;
        let relative = store.relative(file);
        let folder = match relative.rsplit_once('/') {
            Some((folder, _)) => format!("{folder}/"),
            None => "/".to_string(),
        };
        // Forgotten and looked up at once, so a design is either kept already
        // and found here, or not yet kept and made again by `refresh`.
        let (affected, forgotten): (Vec<(String, Resolved)>, Vec<String>) = {
            let resolved = lock(&store.resolved);
            let mut sources = lock(&store.sources);
            let forgotten = sources
                .iter()
                .filter(|(key, source)| {
                    [relative.as_str(), folder.as_str()].iter().any(|changed| {
                        key == changed || source.canonical.as_deref() == Some(changed)
                    })
                })
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            if forgotten.is_empty() {
                return;
            }
            for key in &forgotten {
                sources.remove(key);
            }
            let affected = resolved
                .iter()
                .filter(|(_, resolved)| resolved.asked.iter().any(|a| forgotten.contains(a)))
                .map(|(path, resolved)| (path.clone(), resolved.clone()))
                .collect();
            (affected, forgotten)
        };
        for (path, old) in affected {
            let new = store.refresh(&path, &forgotten);
            let differs = new.version != old.version
                || new.problems != old.problems
                || new.missing != old.missing;
            if differs {
                for listener in lock(&store.listeners).iter() {
                    listener(&path);
                }
            }
        }
        // A design file no design reads any more takes its last parse with
        // it.
        let resolved = lock(&store.resolved);
        let mut syntax = lock(&store.syntax);
        for key in &forgotten {
            if syntax.contains_key(key) && !resolved.values().any(|r| r.asked.contains(key)) {
                syntax.remove(key);
            }
        }
    }

    /// The validator of the design at `path`'s PNG in `theme`, known before
    /// painting: the version of its scene — one more for every change to it
    /// — of this store, the theme, and devpage's build, which paints it.
    /// `None` when it doesn't resolve. It may resolve the design, so it
    /// blocks.
    pub fn etag(&self, path: &str, theme: Theme) -> Option<String> {
        let path = address(path);
        let resolved = self.0.get(path);
        if resolved.problems.is_some() {
            return None;
        }
        let text = format!(
            "{}\0{theme:?}\0{path}\0{}\0{}",
            crate::addresses::BUILD,
            self.0.nonce,
            resolved.version
        );
        Some(crate::addresses::hash(text.as_bytes()))
    }

    /// The design at `path` as a PNG in `theme`, written to `out` band by
    /// band — each band at most 16 MB of pixels: one row of tiles at 1×, from
    /// the store's cache, only the missing and stale ones painted, or for a
    /// drawing too wide for that, shorter strips painted and not cached — or its problems one per line,
    /// before anything is written. Painting blocks; a writer that fails — a
    /// client gone — stops it.
    ///
    /// No lock of the store's is held while `out` is written to, so a client
    /// that stops reading holds up nothing but its own PNG. A save that
    /// changes the design while its PNG is written ends the PNG there, with
    /// an error, rather than finish it from another scene.
    pub fn png_to(&self, path: &str, theme: Theme, out: impl std::io::Write) -> Result<(), String> {
        let path = address(path);
        let store = &self.0;
        let resolved = store.get(path);
        if let Some(problems) = &resolved.problems {
            return Err(lines(problems));
        }
        let kept = store.kept(path);
        let (id, version, area, theme) = {
            let k = lock(&kept);
            if let Some(problems) = &k.problems {
                return Err(lines(problems));
            }
            let themed = k.resolution.as_ref().is_some_and(|r| r.scene().themed);
            let theme = if themed { theme } else { Theme::Light };
            let drawing = k
                .drawings
                .iter()
                .find(|d| d.theme() == theme)
                .ok_or_else(|| format!("{path}: the design has no drawing in {theme:?}"))?;
            (k.id, k.version, drawing.area(), theme)
        };
        guarded(
            path,
            || {
                devpage_design::bands::bands(area.width as u32, area.height as u32, out, |piece| {
                    let (col, row) = match piece {
                        Piece::Tile { col, row } => (col, row),
                        // A band of a drawing too wide for whole tiles is
                        // painted in strips, never cached.
                        Piece::Strip { col, y, rows } => {
                            let mut kept = lock(&kept);
                            let drawing =
                                prepared(&mut kept, version, theme, &store.renderer, |d| {
                                    let a = d.area();
                                    let (x, y) = (
                                        a.x as f64 + (col * devpage_design::tiles::TILE) as f64,
                                        a.y as f64 + y as f64,
                                    );
                                    devpage_design::KRect::new(
                                        x,
                                        y,
                                        x + devpage_design::tiles::TILE as f64,
                                        y + rows as f64,
                                    )
                                })?;
                            return Ok::<_, &str>(
                                devpage_design::cpu::strip(&drawing.list, col, y, rows).into(),
                            );
                        }
                    };
                    let tile = Tile {
                        design: id,
                        key: Key {
                            theme,
                            level: Level::of(1.0),
                            col,
                            row,
                        },
                    };
                    // The design's lock, for one tile at a time: never while
                    // the band is written.
                    let mut kept = lock(&kept);
                    if Some(&*kept).filter(|k| k.version == version).is_none() {
                        return Err("the design changed while its PNG was written");
                    }
                    if let Some((pixels, false)) = lock(&store.tiles).get(&tile) {
                        return Ok(pixels.clone());
                    }
                    let drawing = prepared(&mut kept, version, theme, &store.renderer, |d| {
                        devpage_design::tiles::tile_rect(&d.list, 1.0, col, row)
                    })?;
                    let pixels: Arc<[u8]> =
                        devpage_design::cpu::tile(&drawing.list, 1.0, col, row).into();
                    #[cfg(test)]
                    store
                        .painted
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    lock(&store.tiles).insert(tile, pixels.clone(), TILE_BYTES);
                    Ok(pixels)
                })
                .map_err(|e| format!("writing the PNG: {e}"))
            },
            |p| p.message,
        )
        .map_err(|e| format!("{path}: {e}"))
    }

    /// The design at `path` painted as a PNG in `theme`, or its problems one
    /// per line: [`Designs::png_to`], whole. Painting blocks.
    pub fn png(&self, path: &str, theme: Theme) -> Result<Vec<u8>, String> {
        let mut png = Vec::new();
        self.png_to(path, theme, &mut png)?;
        Ok(png)
    }
}

/// The drawing of `kept` in `theme` — if it is still `version`'s — with
/// what the area `rect` picks rounded where its runs now place it, so what
/// is painted there is a fresh drawing's.
fn prepared<'a>(
    kept: &'a mut Kept,
    version: u64,
    theme: Theme,
    renderer: &Mutex<Renderer>,
    rect: impl FnOnce(&devpage_design::Drawing) -> devpage_design::KRect,
) -> Result<&'a devpage_design::Drawing, &'static str> {
    let gone = "the design changed while its PNG was written";
    if kept.version != version {
        return Err(gone);
    }
    let Kept {
        resolution,
        drawings,
        ..
    } = kept;
    let scene = resolution.as_ref().ok_or(gone)?.scene();
    let drawing = drawings
        .iter_mut()
        .find(|d| d.theme() == theme)
        .ok_or(gone)?;
    let area = rect(drawing);
    drawing.prepare(&mut lock(renderer), scene, area);
    Ok(drawing)
}

/// The design at `path`'s problems, read straight from disk: what
/// `devpage check` and a page's problems bar report.
pub fn problems(served: &ServedFolder, path: &str) -> Vec<Problem> {
    struct Disk<'a>(&'a ServedFolder);
    impl Files for Disk<'_> {
        fn bytes(&self, path: &str) -> Option<Vec<u8>> {
            let file = self.0.inside(&self.0.path().join(path))?;
            std::fs::read(file).ok()
        }
        fn list(&self, path: &str) -> Option<Vec<String>> {
            names(&self.0.folder(&self.0.path().join(path))?)
        }
    }
    let path = address(path);
    guarded(
        path,
        || devpage_design::resolve(&Disk(served), path),
        |p| vec![p],
    )
    .and_then(|scene| drawable(&mut Renderer::new(), path, &scene))
    .err()
    .unwrap_or_default()
}

/// Every file the design at `path` is made of, read straight from disk: the
/// design itself, its imports, images, themes and fonts — what resolving it
/// reads, whether or not it resolves. Folders it lists are not files; the
/// fonts it then reads from them are.
pub fn files(served: &ServedFolder, path: &str) -> Vec<PathBuf> {
    struct Recording<'a> {
        served: &'a ServedFolder,
        read: RefCell<Vec<PathBuf>>,
    }
    impl Files for Recording<'_> {
        fn bytes(&self, path: &str) -> Option<Vec<u8>> {
            let file = self.served.file(&self.served.path().join(path))?;
            let bytes = std::fs::read(&file).ok();
            self.read.borrow_mut().push(file);
            bytes
        }
        fn list(&self, path: &str) -> Option<Vec<String>> {
            names(&self.served.folder(&self.served.path().join(path))?)
        }
    }
    let recording = Recording {
        served,
        read: RefCell::new(Vec::new()),
    };
    let path = address(path);
    let _ = guarded(
        path,
        || devpage_design::resolve(&recording, path),
        |p| vec![p],
    );
    recording.read.into_inner()
}

/// The themes a design shows in: both when it is themed, else the light one,
/// which is the same as the dark.
fn themes(scene: &Scene) -> &'static [Theme] {
    if scene.themed {
        &[Theme::Light, Theme::Dark]
    } else {
        &[Theme::Light]
    }
}

/// Whether the design at `path` can be drawn at all — in each theme, when
/// it is themed: images that don't fit the GPU's atlas are the design's
/// problem everywhere — on its canvas, in its PNG and in `devpage check` —
/// not only where a picture is asked for. What `devpage check` asks of a
/// design read from disk; the store keeps the drawings it makes instead.
fn drawable(renderer: &mut Renderer, path: &str, scene: &Scene) -> Result<(), Vec<Problem>> {
    guarded(
        path,
        || {
            themes(scene)
                .iter()
                .try_for_each(|&theme| renderer.drawing(scene, theme, None).map(|_| ()))
        },
        |p| Undrawable {
            message: p.message,
            at: None,
        },
    )
    .map_err(|undrawable| problem(path, undrawable))
}

/// Why a design can't be drawn, as its problem: at the attribute that broke
/// it, or else at the design's start.
fn problem(path: &str, undrawable: Undrawable) -> Vec<Problem> {
    let (file, pos) = match undrawable.at {
        Some(at) => (at.file, at.pos),
        None => (
            path.to_string(),
            devpage_design::parse::Pos { line: 1, col: 1 },
        ),
    };
    vec![Problem {
        file,
        pos,
        message: undrawable.message,
    }]
}

/// What `f` gives, or — should the engine ever panic on a design — a problem
/// saying so, so that no design file can take the server or its watcher down.
pub fn guarded<T, E>(
    path: &str,
    f: impl FnOnce() -> Result<T, E>,
    failed: impl FnOnce(Problem) -> E,
) -> Result<T, E> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|panic| {
        let message = panic
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        Err(failed(Problem {
            file: path.to_string(),
            pos: devpage_design::parse::Pos { line: 1, col: 1 },
            message: format!("the design engine failed on this design: {message}"),
        }))
    })
}

fn lines(problems: &[Problem]) -> String {
    problems
        .iter()
        .map(Problem::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Designs to test with, and saving one as the watcher would.
#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use std::path::{Path, PathBuf};

    use super::Designs;

    pub const BUTTON: &str = r##"<Frame props={{ label: "Button" }} layout="row" padding={[12, 20]} fill="#2563eb">
  <Text color="#ffffff">{label}</Text>
</Frame>
"##;
    pub const SCREEN: &str = r##"import Button from "./button.design"

<Frame width={200} height={120} layout="column" gap={8} padding={16} fill="#f8fafc">
  <Text>Cart</Text>
  <Button label="Pay" />
</Frame>
"##;
    pub const OTHER: &str = r##"<Frame width={50} height={50} fill="#000000" />
"##;

    pub fn served(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("devpage-designs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("app")).unwrap();
        std::fs::write(dir.join("app/button.design"), BUTTON).unwrap();
        std::fs::write(dir.join("app/screen.design"), SCREEN).unwrap();
        std::fs::write(dir.join("app/other.design"), OTHER).unwrap();
        dir.canonicalize().unwrap()
    }

    pub const THEMES: &str = "kind: themes\nlight:\n  bg: \"#ffffff\"\ndark:\n  bg: \"#000000\"\n";

    /// Writes a file and tells the store, as the watcher does.
    pub fn save(designs: &Designs, file: &Path, text: &str) {
        std::fs::write(file, text).unwrap();
        designs.changed(file);
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    /// What the store tells, as it tells it.
    fn announced(designs: &Designs) -> std::sync::mpsc::Receiver<String> {
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = Mutex::new(tx);
        designs.on_change(move |path| {
            let _ = lock(&tx).send(path.to_string());
        });
        rx
    }

    /// A design 100,000 pixels tall: a column of 1,000 rows, 400 by 100, the
    /// row `edited` set apart by its fill.
    fn tall(dir: &Path, edited: Option<usize>) {
        let rows: String = (0..1000)
            .map(|i| {
                let fill = if Some(i) == edited { 0xff0000 } else { i * 997 };
                format!("<Frame width={{400}} height={{100}} fill=\"#{fill:06x}\" />")
            })
            .collect();
        save_quietly(
            &dir.join("app/huge.design"),
            &format!("<Frame layout=\"column\">{rows}</Frame>"),
        );
    }

    fn save_quietly(file: &Path, text: &str) {
        std::fs::write(file, text).unwrap();
    }

    fn painted(designs: &Designs) -> usize {
        designs
            .0
            .painted
            .swap(0, std::sync::atomic::Ordering::Relaxed)
    }

    /// Counts what is written.
    #[derive(Default)]
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0 += b.len();
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// A design too wide for a row of tiles in one band is banded in strips,
    /// painted and never cached: its PNG is right and the cache stays empty.
    #[test]
    fn a_design_too_wide_for_tiles_is_banded_in_strips_never_cached() {
        let dir = served("wide");
        std::fs::write(
            dir.join("app/wide.design"),
            r##"<Frame width={40000} height={20} fill="#ffffff"><Rect x={39990} y={10} width={10} height={10} fill="#0000ff" /></Frame>"##,
        )
        .unwrap();
        let designs = Designs::new(&ServedFolder::new(&dir).unwrap());
        let png = designs.png("app/wide.design", Theme::Light).unwrap();
        let (px, w, h) = devpage_design::compare::pixels(&png).unwrap();
        assert_eq!((w, h), (40_000, 20));
        let at = |x: usize, y: usize| px[(y * 40_000 + x) * 4..][..4].to_vec();
        assert_eq!(at(0, 0), vec![255, 255, 255, 255]);
        assert_eq!(at(39_999, 19), vec![0, 0, 255, 255]);
        assert_eq!(painted(&designs), 0);
        assert_eq!(lock(&designs.0.tiles).cost(), 0);
    }

    /// A design 100,000 pixels tall has a PNG, streamed band by band from the
    /// store's cache, which never holds more than its cap; asked for again,
    /// only the tiles the cap dropped are painted again.
    #[test]
    fn a_design_100000_pixels_tall_streams_its_png_from_a_capped_cache() {
        let dir = served("huge");
        tall(&dir, None);
        let designs = Designs::new(&ServedFolder::new(&dir).unwrap());
        assert!(problems(&ServedFolder::new(&dir).unwrap(), "/app/huge.design").is_empty());
        let started = std::time::Instant::now();
        let png = designs.png("app/huge.design", Theme::Light).unwrap();
        let took = started.elapsed();
        let (px, w, h) = devpage_design::compare::pixels(&png).unwrap();
        assert_eq!((w, h), (400, 100_000));
        let c = 999 * 997;
        let at = (99_950 * 400 + 200) * 4;
        assert_eq!(&px[at..at + 3], &[(c >> 16) as u8, (c >> 8) as u8, c as u8]);
        // 391 rows of 2 tiles, all painted once; the cache holds 256 of them.
        assert_eq!(painted(&designs), 782);
        let held = lock(&designs.0.tiles).cost();
        assert!(held <= TILES_CAP, "{held}");
        assert_eq!(held, TILES_CAP);
        eprintln!("100,000-px PNG: {took:?}, {} bytes", png.len());
        let mut count = Count::default();
        designs
            .png_to("app/huge.design", Theme::Light, &mut count)
            .unwrap();
        assert_eq!(count.0, png.len());
        // The last bands were cached; the first ones were dropped for them,
        // and dropping them again as the PNG goes on, all are painted again.
        assert_eq!(painted(&designs), 782);
        assert!(lock(&designs.0.tiles).cost() <= TILES_CAP);
    }

    /// A PNG asked for twice paints nothing the second time; after an edit to
    /// one node of a design 100,000 pixels tall, only the tiles over it.
    #[test]
    fn an_edit_repaints_only_the_tiles_over_it() {
        let dir = served("edit");
        std::fs::write(
            dir.join("app/small.design"),
            r##"<Frame width={600} height={300} fill="#336699" />"##,
        )
        .unwrap();
        let designs = Designs::new(&ServedFolder::new(&dir).unwrap());
        let first = designs.png("app/small.design", Theme::Light).unwrap();
        assert_eq!(painted(&designs), 6);
        assert_eq!(
            designs.png("app/small.design", Theme::Light).unwrap(),
            first
        );
        assert_eq!(painted(&designs), 0);

        tall(&dir, None);
        // Room for the whole design, to count only what the edit needs.
        *lock(&designs.0.tiles) = Cache::new(usize::MAX);
        let before = designs.png("app/huge.design", Theme::Light).unwrap();
        assert_eq!(painted(&designs), 782);
        // Row 500 is y 50,000 to 50,100, inside tile row 195: its two tiles.
        tall(&dir, Some(500));
        designs.changed(&dir.join("app/huge.design"));
        let started = std::time::Instant::now();
        let after = designs.png("app/huge.design", Theme::Light).unwrap();
        let took = started.elapsed();
        assert_eq!(painted(&designs), 2);
        eprintln!("PNG after an edit: {took:?}, 2 tiles painted");
        let (a, _, _) = devpage_design::compare::pixels(&before).unwrap();
        let (b, _, _) = devpage_design::compare::pixels(&after).unwrap();
        let row = |px: &[u8], y: usize| px[(y * 400 + 10) * 4..(y * 400 + 10) * 4 + 4].to_vec();
        assert_eq!(row(&b, 50_050), vec![255, 0, 0, 255]);
        assert_eq!(row(&a, 49_950), row(&b, 49_950));
        assert_eq!(row(&a, 50_150), row(&b, 50_150));
        // The edit undone: those tiles again, and the PNG as before.
        tall(&dir, None);
        designs.changed(&dir.join("app/huge.design"));
        assert_eq!(
            designs.png("app/huge.design", Theme::Light).unwrap(),
            before
        );
        assert_eq!(painted(&designs), 2);
    }

    /// A themed design keeps a drawing in each theme, and each theme its own
    /// tiles; a design that stops using its themes drops its dark ones.
    #[test]
    fn each_theme_keeps_its_own_tiles() {
        let dir = served("theme-tiles");
        std::fs::write(dir.join("app/themes.yml"), THEMES).unwrap();
        let file = dir.join("app/t.design");
        std::fs::write(&file, "<Frame width={300} height={100} fill={theme.bg} />").unwrap();
        let designs = Designs::new(&ServedFolder::new(&dir).unwrap());
        let light = designs.png("app/t.design", Theme::Light).unwrap();
        let dark = designs.png("app/t.design", Theme::Dark).unwrap();
        assert_ne!(light, dark);
        assert_eq!(painted(&designs), 4);
        designs.png("app/t.design", Theme::Light).unwrap();
        designs.png("app/t.design", Theme::Dark).unwrap();
        assert_eq!(painted(&designs), 0);
        std::fs::write(
            &file,
            r##"<Frame width={300} height={100} fill="#00ff00" />"##,
        )
        .unwrap();
        designs.changed(&file);
        assert!(
            lock(&designs.0.tiles)
                .iter()
                .all(|(t, _, _)| t.key.theme == Theme::Light)
        );
        // Not themed: the dark PNG is the light one.
        let green = designs.png("app/t.design", Theme::Light).unwrap();
        assert_eq!(designs.png("app/t.design", Theme::Dark).unwrap(), green);
    }

    /// A PNG whose reader has stopped reading holds up no save: the store
    /// takes the save while the PNG waits, and the PNG, once read again,
    /// ends rather than finish from the next scene.
    #[test]
    fn a_save_lands_while_a_png_is_not_read() {
        let dir = served("unread");
        tall(&dir, None);
        let designs = Designs::new(&ServedFolder::new(&dir).unwrap());
        designs.resolve("app/huge.design").unwrap();
        /// A reader that stops after the first write until it is let go.
        struct Stalled(std::sync::mpsc::Receiver<()>, bool);
        impl std::io::Write for Stalled {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                if self.1 {
                    self.1 = false;
                    let _ = self.0.recv();
                }
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let (go, wait) = std::sync::mpsc::channel();
        let reader = designs.clone();
        let png = std::thread::spawn(move || {
            reader.png_to("app/huge.design", Theme::Light, Stalled(wait, true))
        });
        // Let the PNG reach its first write.
        std::thread::sleep(std::time::Duration::from_millis(200));
        let (done, saved) = std::sync::mpsc::channel();
        let saver = designs.clone();
        let file = dir.join("app/huge.design");
        let save_dir = dir.clone();
        std::thread::spawn(move || {
            tall(&save_dir, Some(3));
            saver.changed(&file);
            let _ = done.send(());
        });
        assert!(
            saved
                .recv_timeout(std::time::Duration::from_secs(20))
                .is_ok(),
            "the save waited for the PNG's reader"
        );
        go.send(()).unwrap();
        let ended = png.join().unwrap().unwrap_err();
        assert!(
            ended.contains("changed while its PNG was written"),
            "{ended}"
        );
        // The next PNG is the new scene's.
        let next = designs.png("app/huge.design", Theme::Light).unwrap();
        let (px, _, _) = devpage_design::compare::pixels(&next).unwrap();
        let at = (350 * 400 + 10) * 4;
        assert_eq!(&px[at..at + 4], &[255, 0, 0, 255]);
    }

    /// A writer that fails — a client gone — stops the painting there.
    #[test]
    fn a_writer_that_fails_stops_the_painting() {
        let dir = served("gone");
        tall(&dir, None);
        let designs = Designs::new(&ServedFolder::new(&dir).unwrap());
        struct Gone;
        impl std::io::Write for Gone {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("gone"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        assert!(
            designs
                .png_to("app/huge.design", Theme::Light, Gone)
                .is_err()
        );
        assert!(painted(&designs) <= 2, "painted past the first band");
    }

    /// Images that don't fit the GPU's atlas together are a problem at the
    /// `src` of the first that doesn't, wherever the design is looked at.
    #[test]
    fn images_too_many_for_the_gpu_are_a_problem_at_their_src() {
        /// `w`x`h`-name.png is an image file that size, never decoded.
        struct Sized(String);
        impl Files for Sized {
            fn bytes(&self, path: &str) -> Option<Vec<u8>> {
                (path == "app/a.design").then(|| self.0.clone().into_bytes())
            }
            fn list(&self, _: &str) -> Option<Vec<String>> {
                None
            }
            fn image(&self, path: &str) -> Option<Result<Arc<ImageFile>, String>> {
                let (size, _) = path.strip_prefix("app/")?.split_once('-')?;
                let (w, h) = size.split_once('x')?;
                let (w, h): (u32, u32) = (w.parse().ok()?, h.parse().ok()?);
                Some(Ok(Arc::new(ImageFile {
                    id: devpage_design::resolve::ImageId::of(path.as_bytes()),
                    bytes: Vec::new(),
                    width: w,
                    height: h,
                    pixels: Arc::new(vec![0; 4 * w as usize * h as usize]),
                    pixels_width: w,
                    pixels_height: h,
                })))
            }
        }
        let images: String = "abcde"
            .chars()
            .map(|c| format!("\n  <Image src=\"4096x4096-{c}.png\" width={{40}} height={{40}} />"))
            .collect();
        let design = Sized(format!(r#"<Frame layout="row">{images}</Frame>"#));
        let scene = devpage_design::resolve(&design, "app/a.design").unwrap();
        let found = drawable(&mut Renderer::new(), "app/a.design", &scene).unwrap_err();
        assert_eq!(found.len(), 1);
        assert!(
            found[0]
                .to_string()
                .starts_with("app/a.design:6:10: this doesn't fit: "),
            "{found:?}"
        );
    }

    /// An image file is decoded once per read of it: a save of the design
    /// showing it, or of another file, doesn't decode it again.
    #[test]
    fn a_save_decodes_only_the_image_that_changed() {
        let dir = served("decode-once");
        let shirt = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/designs/checkout/shirt.jpg"),
        )
        .unwrap();
        std::fs::write(dir.join("app/p.jpg"), &shirt).unwrap();
        let photo = "<Frame>\n  <Image src=\"./p.jpg\" />\n  <Image src=\"./p.jpg\" />\n</Frame>\n";
        std::fs::write(dir.join("app/photo.design"), photo).unwrap();
        let designs = Designs::new(&ServedFolder::new(&dir).unwrap());
        let first = designs.resolve("app/photo.design").unwrap();
        assert_eq!(first.images.len(), 1, "one file shown twice");
        save(
            &designs,
            &dir.join("app/photo.design"),
            &photo.replace("<Frame>", "<Frame fill=\"#ffffff\">"),
        );
        let second = designs.resolve("app/photo.design").unwrap();
        assert_ne!(first.root, second.root);
        let file = |s: &Scene| s.images.values().next().unwrap().clone();
        assert!(Arc::ptr_eq(&file(&first), &file(&second)));
        // A saved image file is decoded anew; the scene keeps the decode it
        // has while the bytes are the same.
        std::fs::write(dir.join("app/p.jpg"), &shirt).unwrap();
        designs.changed(&dir.join("app/p.jpg"));
        let third = designs.resolve("app/photo.design").unwrap();
        let decoded = lock(&designs.0.sources)["app/p.jpg"]
            .image
            .clone()
            .unwrap()
            .unwrap();
        assert!(!Arc::ptr_eq(&file(&second), &decoded));
        assert_eq!(file(&third).id, decoded.id);
    }

    #[test]
    fn an_image_that_does_not_decode_is_a_problem_everywhere() {
        let dir = served("broken-image");
        let mut broken = b"\x89PNG\r\n\x1a\n".to_vec();
        broken.extend(b"garbage");
        std::fs::write(dir.join("app/broken.png"), broken).unwrap();
        std::fs::write(
            dir.join("app/photo.design"),
            "<Frame>\n  <Image src=\"./broken.png\" width={50} height={50} />\n</Frame>\n",
        )
        .unwrap();
        let expected = "app/photo.design:2:10: src: ./broken.png is a PNG that doesn't decode";
        // What `devpage check` reports…
        let found = problems(&ServedFolder::new(&dir).unwrap(), "app/photo.design");
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].to_string().starts_with(expected), "{found:?}");
        // …and the PNG, instead of a blank picture.
        let designs = Designs::new(&ServedFolder::new(&dir).unwrap());
        let png = designs.png("app/photo.design", Theme::Light).unwrap_err();
        assert!(png.starts_with(expected), "{png}");
    }

    #[test]
    fn only_the_designs_made_from_a_changed_file_are_announced() {
        let dir = served("store");
        let designs = Designs::new(&ServedFolder::new(&dir).unwrap());
        let updates = announced(&designs);
        assert!(designs.resolve("app/screen.design").is_ok());
        assert!(designs.resolve("/app/other.design").is_ok());
        let resolutions = || {
            designs
                .0
                .resolutions
                .load(std::sync::atomic::Ordering::Relaxed)
        };
        assert_eq!(resolutions(), 2);
        assert!(designs.resolve("app/screen.design").is_ok());
        assert_eq!(resolutions(), 2, "kept in memory");

        save(
            &designs,
            &dir.join("app/button.design"),
            &BUTTON.replace("#2563eb", "#16a34a"),
        );
        assert_eq!(updates.try_recv().unwrap(), "app/screen.design");
        assert!(
            updates.try_recv().is_err(),
            "other.design doesn't use the button"
        );
        assert_eq!(resolutions(), 3, "only screen.design was resolved again");

        // Saving the same scene again announces nothing.
        save(
            &designs,
            &dir.join("app/button.design"),
            &BUTTON.replace("#2563eb", "#16a34a"),
        );
        assert!(updates.try_recv().is_err());

        // A file nothing was made from is not even looked at.
        save(&designs, &dir.join("app/notes.txt"), "hi");
        assert!(updates.try_recv().is_err());
    }

    /// A save that lands while a design is being resolved — after its file
    /// was read, before the result is kept — is not lost: the stale result
    /// is never kept, and the design is resolved from the new file.
    #[test]
    fn a_save_while_a_design_resolves_is_not_lost() {
        let dir = served("race");
        let designs = Designs::new(&ServedFolder::new(&dir).unwrap());
        let other = dir.join("app/other.design");
        let saver = designs.clone();
        let saved = std::sync::atomic::AtomicBool::new(false);
        *lock(&designs.0.after_read) = Some(Box::new(move |path: &str| {
            if path == "app/other.design" && !saved.swap(true, std::sync::atomic::Ordering::SeqCst)
            {
                save(&saver, &other, &OTHER.replace("{50}", "{20}"));
            }
        }));
        let width =
            |designs: &Designs| match designs.resolve("app/other.design").unwrap().root.width {
                devpage_design::resolve::Size::Px(w) => w,
                other => panic!("{other:?}"),
            };
        assert_eq!(width(&designs), 20.0, "resolved from the saved file");
        *lock(&designs.0.after_read) = None;
        assert_eq!(width(&designs), 20.0, "and kept so");
    }

    #[test]
    fn no_design_file_stops_the_store() {
        let dir = served("hostile");
        let designs = Designs::new(&ServedFolder::new(&dir).unwrap());
        let other = dir.join("app/other.design");
        assert!(designs.png("app/other.design", Theme::Light).is_ok());
        // A colour with a pasted zero-width space, saved over a design held
        // in memory: what once panicked the watcher's thread.
        save(&designs, &other, &OTHER.replace("#000000", "#000\u{200b}"));
        let problems = designs.png("app/other.design", Theme::Light).unwrap_err();
        assert!(
            problems.contains("fill: #000\u{200b} is not a color"),
            "{problems}"
        );
        // Deep nesting, a huge number, a truncated file: problems, all.
        for text in [
            "<Frame>".repeat(5000),
            "<Frame width={99999999999} />".to_string(),
            "<Frame fill=\"#ff".to_string(),
        ] {
            save(&designs, &other, &text);
            assert!(
                designs.png("app/other.design", Theme::Light).is_err(),
                "{text:.40}"
            );
        }
        // And the store still sees saves afterwards.
        save(&designs, &other, &OTHER.replace("{50}", "{20}"));
        let png = designs.png("app/other.design", Theme::Light).unwrap();
        let (_, w, h) = devpage_design::compare::pixels(&png).unwrap();
        assert_eq!((w, h), (20, 20));
    }

    #[test]
    fn creating_a_missing_import_resolves_the_design_again() {
        let dir = served("create");
        let designs = Designs::new(&ServedFolder::new(&dir).unwrap());
        std::fs::remove_file(dir.join("app/button.design")).unwrap();
        let problems = designs.resolve("app/screen.design").unwrap_err();
        assert!(problems[0].message.contains("no such file"), "{problems:?}");
        save(&designs, &dir.join("app/button.design"), BUTTON);
        assert!(designs.resolve("app/screen.design").is_ok());
    }

    /// Creating, editing and deleting a `themes.yml` resolves again exactly
    /// the designs that read it — a missing one included.
    #[test]
    fn a_themes_file_resolves_again_only_the_designs_that_read_it() {
        let dir = served("themes");
        std::fs::write(
            dir.join("app/themed.design"),
            "<Frame width={10} height={10} fill={theme.bg} />",
        )
        .unwrap();
        let designs = Designs::new(&ServedFolder::new(&dir).unwrap());
        let updates = announced(&designs);
        let problems = designs.resolve("app/themed.design").unwrap_err();
        assert!(
            problems[0].message.contains("no themes.yml"),
            "{problems:?}"
        );
        assert!(designs.resolve("app/other.design").is_ok());
        let fill = |designs: &Designs| designs.resolve("app/themed.design").unwrap().root.fill;
        // Created at the top: two folders above the design.
        save(&designs, &dir.join("themes.yml"), THEMES);
        assert_eq!(updates.try_recv().unwrap(), "app/themed.design");
        assert!(updates.try_recv().is_err(), "other.design reads no themes");
        let top = fill(&designs);
        // Created between them: it overrides the top one.
        save(
            &designs,
            &dir.join("app/themes.yml"),
            &THEMES.replace("#000000", "#111111"),
        );
        assert_eq!(updates.try_recv().unwrap(), "app/themed.design");
        assert_ne!(fill(&designs), top);
        // Deleted: the top one again.
        std::fs::remove_file(dir.join("app/themes.yml")).unwrap();
        designs.changed(&dir.join("app/themes.yml"));
        assert_eq!(updates.try_recv().unwrap(), "app/themed.design");
        assert_eq!(fill(&designs), top);
        assert!(updates.try_recv().is_err());
    }

    /// A font is one of the files of the designs set in it: a change to a
    /// font file, or to any file in a font folder, resolves again exactly
    /// the designs whose text is set in it — `devpage check` reports the
    /// same — and the scene carries the face its text uses.
    #[test]
    fn a_font_change_resolves_again_only_the_designs_set_in_it() {
        let dir = served("fonts");
        let regular = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../design/fonts/Inter-Regular.ttf"),
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("app/fonts/Body")).unwrap();
        std::fs::write(dir.join("app/fonts/Body/Body-Regular.ttf"), &regular).unwrap();
        std::fs::write(dir.join("app/fonts/Body/OFL.txt"), "licence").unwrap();
        std::fs::write(
            dir.join("app/themes.yml"),
            "kind: themes\nfonts:\n  body: ./fonts/Body\nlight: {}\ndark: {}\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("app/set.design"),
            r#"<Frame><Text font="body">Hello</Text></Frame>"#,
        )
        .unwrap();
        let designs = Designs::new(&ServedFolder::new(&dir).unwrap());
        let updates = announced(&designs);
        let scene = designs.resolve("app/set.design").unwrap();
        assert_eq!(scene.fonts.len(), 1);
        assert!(designs.resolve("app/other.design").is_ok());
        let resolutions = || {
            designs
                .0
                .resolutions
                .load(std::sync::atomic::Ordering::Relaxed)
        };
        let before = resolutions();
        // The licence changes: the folder's listing is the same, the design
        // is resolved again and unchanged.
        save(&designs, &dir.join("app/fonts/Body/OFL.txt"), "licence, v2");
        assert_eq!(resolutions(), before + 1, "only set.design");
        assert!(updates.try_recv().is_err());
        // A second regular face arrives: a problem at the themes' font.
        let mut copy = regular.clone();
        copy.push(0);
        std::fs::write(dir.join("app/fonts/Body/Copy.ttf"), &copy).unwrap();
        designs.changed(&dir.join("app/fonts/Body/Copy.ttf"));
        assert_eq!(updates.try_recv().unwrap(), "app/set.design");
        assert!(updates.try_recv().is_err());
        let expected = "app/themes.yml:3:3: fonts.body: ./fonts/Body holds two files of weight 400: Body-Regular.ttf and Copy.ttf";
        let problems = designs.resolve("app/set.design").unwrap_err();
        assert_eq!(problems[0].to_string(), expected);
        assert_eq!(
            super::problems(&ServedFolder::new(&dir).unwrap(), "/app/set.design")[0].to_string(),
            expected
        );
        // It goes again, and a font file itself is saved over as bold.
        std::fs::remove_file(dir.join("app/fonts/Body/Copy.ttf")).unwrap();
        designs.changed(&dir.join("app/fonts/Body/Copy.ttf"));
        assert_eq!(updates.try_recv().unwrap(), "app/set.design");
        let bold = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../design/fonts/Inter-Bold.ttf"),
        )
        .unwrap();
        std::fs::write(dir.join("app/fonts/Body/Body-Regular.ttf"), &bold).unwrap();
        designs.changed(&dir.join("app/fonts/Body/Body-Regular.ttf"));
        assert_eq!(updates.try_recv().unwrap(), "app/set.design");
        assert_eq!(
            designs.resolve("app/set.design").unwrap_err()[0].message,
            "weight: Inter has no 400; it has 700"
        );
        assert!(updates.try_recv().is_err());
    }

    #[test]
    fn a_design_outside_the_served_folder_is_never_read() {
        let dir = served("outside");
        std::fs::write(dir.join("../outside.design"), OTHER).unwrap();
        let designs = Designs::new(&ServedFolder::new(&dir.join("app")).unwrap());
        assert!(designs.resolve("../outside.design").is_err());
    }
    /// Saves through the store — rows inserted at the front and the end,
    /// edited, removed, a component and a theme changed, a break and its fix
    /// — each sent to two streams, one joining late, as the store's one
    /// patch: the browser's copy is a fresh resolve of the files every time.
    #[test]
    fn a_save_is_sent_as_what_it_reached() {
        let dir = served("reached");
        std::fs::write(dir.join("app/themes.yml"), THEMES).unwrap();
        let screen = |rows: &[&str]| {
            let rows: String = rows
                .iter()
                .map(|r| format!("  <Frame fill={{theme.bg}}><Text>{r}</Text></Frame>\n"))
                .collect();
            format!(
                "import Button from \"./button.design\"\n<Frame layout=\"column\">\n{rows}  <Button label=\"Pay\" />\n</Frame>\n"
            )
        };
        let file = dir.join("app/list.design");
        let mut rows = vec!["a", "b", "c"];
        std::fs::write(&file, screen(&rows)).unwrap();
        let designs = Designs::new(&ServedFolder::new(&dir).unwrap());
        let path = "app/list.design";
        let decode = |s: &proto::Scene| {
            devpage_design::wire::decode_scene(s, (&Default::default(), &Default::default()))
                .unwrap()
        };
        let Next::Scene(mut v, scene) = designs.next(path, None) else {
            panic!()
        };
        let mut browser = decode(&scene);
        let mut uses = devpage_design::wire::Uses::of(&browser.root);
        let mut late = None;
        type Edit = Box<dyn Fn(&mut Vec<&'static str>) -> (&'static str, String)>;
        let edits: Vec<Edit> = vec![
            Box::new(|r| {
                r.insert(0, "front");
                ("app/list.design", String::new())
            }),
            Box::new(|r| {
                r.push("end");
                ("app/list.design", String::new())
            }),
            Box::new(|r| {
                r[1] = "edited";
                ("app/list.design", String::new())
            }),
            Box::new(|r| {
                r.remove(2);
                ("app/list.design", String::new())
            }),
            Box::new(|_| ("app/button.design", BUTTON.replace("#2563eb", "#16a34a"))),
            Box::new(|_| ("app/themes.yml", THEMES.replace("#ffffff", "#eeeeee"))),
            Box::new(|_| {
                (
                    "app/button.design",
                    "<Frame fill={theme.nope} />".to_string(),
                )
            }),
            Box::new(|r| {
                r.insert(1, "while broken");
                ("app/list.design", String::new())
            }),
            Box::new(|_| ("app/button.design", BUTTON.to_string())),
        ];
        for (step, edit) in edits.iter().enumerate() {
            let (name, text) = edit(&mut rows);
            let text = if text.is_empty() { screen(&rows) } else { text };
            save(&designs, &dir.join(name), &text);
            match designs.next(path, Some(v)) {
                Next::Problems(_) => continue,
                Next::Patch(now, patch) => {
                    v = now;
                    devpage_design::wire::apply(&mut browser, &mut uses, &patch)
                        .unwrap_or_else(|e| panic!("step {step}: {e}"));
                }
                Next::Current(_) => {}
                Next::Scene(..) => panic!("step {step}: sent whole"),
            }
            // A store that sent the scene by patches holds a fresh resolve.
            let kept = designs.resolve(path).unwrap();
            assert_eq!(
                devpage_design::wire::encode(&browser.root),
                devpage_design::wire::encode(&kept.root),
                "step {step}"
            );
            let fresh = Designs::new(&ServedFolder::new(&dir).unwrap())
                .resolve(path)
                .unwrap();
            assert_eq!(
                kept.root.children.len(),
                fresh.root.children.len(),
                "step {step}"
            );
            if step == 3 {
                late = Some(designs.next(path, None));
            }
        }
        assert!(matches!(late, Some(Next::Scene(..))));
        // Nothing was resolved whole after the first.
        let kept = designs.0.kept(path);
        assert_eq!(lock(&kept).resolution.as_ref().unwrap().wholes(), 0);
    }
}

#[cfg(test)]
mod bench {
    use super::testing::*;
    use super::*;

    /// Save → patch for one row's text, at 1k/10k/100k rows: run with
    /// `cargo test --release -p devpage-core bench -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn save_to_patch() {
        for n in [1_000usize, 10_000, 100_000] {
            let dir = served(&format!("bench-{n}"));
            let file = dir.join("app/list.design");
            let rows = |edited: usize| -> String {
                let body: String = (0..n)
                    .map(|i| {
                        let t = if i == edited {
                            "Edited".to_string()
                        } else {
                            format!("Row {i}")
                        };
                        format!("<Frame width={{400}} height={{20}}><Text>{t}</Text></Frame>\n")
                    })
                    .collect();
                format!("<Frame layout=\"column\">\n{body}</Frame>\n")
            };
            std::fs::write(&file, rows(usize::MAX)).unwrap();
            let designs = Designs::new(&ServedFolder::new(&dir).unwrap());
            let v = match designs.next("app/list.design", None) {
                Next::Scene(v, _) => v,
                _ => panic!(),
            };
            std::fs::write(&file, rows(n / 2)).unwrap();
            // Save to patch: resolving again, the drawing brought across
            // (whose layout is the layout's own cost), and the patch read
            // for a stream.
            let t = std::time::Instant::now();
            designs.changed(&file);
            let store = t.elapsed();
            let t = std::time::Instant::now();
            let Next::Patch(_, patch) = designs.next("app/list.design", Some(v)) else {
                panic!()
            };
            let diff = t.elapsed();
            // Of the store's time, resolving again alone.
            let mut saves = devpage_design::resolve::Resolution::new(
                &std::collections::HashMap::<String, String>::from([(
                    "a.design".to_string(),
                    rows(usize::MAX),
                )]),
                "a.design",
            )
            .unwrap();
            let files = std::collections::HashMap::<String, String>::from([(
                "a.design".to_string(),
                rows(n / 2),
            )]);
            let t = std::time::Instant::now();
            let _ = saves.update(&files, &["a.design".to_string()]);
            let again = t.elapsed();
            // Of the rest, matching the saved file's elements against its
            // last parse — what the store does and the line above doesn't.
            let last = devpage_design::parse::parse(&rows(usize::MAX)).unwrap();
            let t = std::time::Instant::now();
            let fresh = devpage_design::parse::parse(&rows(n / 2)).unwrap();
            let parsed = t.elapsed();
            let t = std::time::Instant::now();
            let _ = devpage_design::ids::matched(fresh, &last);
            let matched = t.elapsed();
            eprintln!(
                "{n}: resolve again {again:?}, of the store's: parse {parsed:?}, match {matched:?}"
            );
            eprintln!(
                "{n}: store {store:?}, stream's patch {diff:?}, {} ops",
                patch.ops.len()
            );
        }
    }
}
