//! The design store: every design asked for kept resolved in memory and
//! resolved again only when a save touched what it read, its problems and
//! its PNG. The watch stream and the canvas page that show designs in the
//! browser are the server's, in `server::watch_stream` and `server::canvas`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use devpage_design::fonts::FontFile;
use devpage_design::resolve::ImageFile;
use devpage_design::{Files, Problem, Renderer, Scene, Theme};

use crate::served::ServedFolder;

/// A design resolved once: its scene or its problems, and every file the
/// resolving asked for — missing ones included, so creating one counts.
#[derive(Clone)]
struct Resolved {
    result: Result<Scene, Vec<Problem>>,
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
    /// Told the address of each design whose scene or problems changed.
    listeners: Mutex<Vec<Listener>>,
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

    /// The design at `path` resolved from the files as they are now, and
    /// kept. A file the resolving read may change while it runs, after the
    /// read and before the result is kept, and `changed` would then find
    /// nothing kept to resolve again; so the result is kept only if every
    /// file it read is still that read, and is made again otherwise.
    fn refresh(&self, path: &str) -> Resolved {
        loop {
            let (resolved, seen) = self.resolve(path);
            let mut kept = lock(&self.resolved);
            let sources = lock(&self.sources);
            let current = seen
                .iter()
                .all(|(file, read)| sources.get(file).is_some_and(|s| s.read == *read));
            if current {
                kept.insert(path.to_string(), resolved.clone());
                return resolved;
            }
        }
    }

    /// The design at `path` resolved, and which read of each file it saw.
    fn resolve(&self, path: &str) -> (Resolved, Vec<(String, u64)>) {
        #[cfg(test)]
        self.resolutions
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let reading = Reading {
            store: self,
            asked: RefCell::new(Vec::new()),
            seen: RefCell::new(Vec::new()),
        };
        let result = guarded(
            path,
            || devpage_design::resolve(&reading, path),
            |p| vec![p],
        )
        .and_then(|scene| {
            drawable(&mut lock(&self.renderer), path, &scene)?;
            Ok(scene)
        });
        let missing = lock(&self.sources)
            .get(path)
            .is_none_or(|source| source.bytes.is_none());
        let resolved = Resolved {
            result,
            asked: reading.asked.into_inner(),
            missing,
        };
        (resolved, reading.seen.into_inner())
    }

    /// The design at `path`, from memory, resolving it the first time.
    fn get(&self, path: &str) -> Resolved {
        if let Some(resolved) = lock(&self.resolved).get(path) {
            return resolved.clone();
        }
        self.refresh(path)
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
    /// The store for the served folder, which must be canonical.
    pub fn new(served: PathBuf) -> Designs {
        Designs(Arc::new(Store {
            served: ServedFolder::canonical(served),
            renderer: Mutex::new(Renderer::new()),
            sources: Mutex::default(),
            reads: Default::default(),
            resolved: Mutex::default(),
            listeners: Mutex::default(),
            #[cfg(test)]
            resolutions: Default::default(),
            #[cfg(test)]
            after_read: Mutex::default(),
        }))
    }

    /// The design at `path` resolved, or its problems one per line.
    pub fn resolve(&self, path: &str) -> Result<Scene, Vec<Problem>> {
        self.0.get(address(path)).result
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
        let affected: Vec<(String, Resolved)> = {
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
            resolved
                .iter()
                .filter(|(_, resolved)| resolved.asked.iter().any(|a| forgotten.contains(a)))
                .map(|(path, resolved)| (path.clone(), resolved.clone()))
                .collect()
        };
        for (path, old) in affected {
            let new = store.refresh(&path);
            let differs = new.result != old.result || new.missing != old.missing;
            if differs {
                for listener in lock(&store.listeners).iter() {
                    listener(&path);
                }
            }
        }
    }

    /// The validator of the design at `path`'s PNG in `theme`, known before
    /// painting: a hash of its resolved scene — every node and the image
    /// files it shows — the theme, and devpage's build, which paints it.
    /// `None` when it doesn't resolve. It may resolve the design, so it
    /// blocks.
    pub fn etag(&self, path: &str, theme: Theme) -> Option<String> {
        let scene = self.resolve(path).ok()?;
        let mut text = format!("{}\0{theme:?}\0{:?}", crate::addresses::BUILD, scene.root);
        for id in scene.images.keys() {
            text.push_str(&format!("\0{id:?}"));
        }
        for id in scene.fonts.keys() {
            text.push_str(&format!("\0{id:?}"));
        }
        Some(crate::addresses::hash(text.as_bytes()))
    }

    /// The design at `path` painted as a PNG in `theme`, or its problems one
    /// per line. Painting blocks.
    pub fn png(&self, path: &str, theme: Theme) -> Result<Vec<u8>, String> {
        let scene = self.resolve(path).map_err(|problems| lines(&problems))?;
        let mut renderer = lock(&self.0.renderer);
        guarded(
            path,
            || devpage_design::png(&mut renderer, &scene, theme),
            |p| p.message,
        )
        .map_err(|e| format!("{}: {e}", address(path)))
    }
}

/// The design at `path`'s problems, read straight from disk: what
/// `devpage check` and a page's problems bar report.
pub fn problems(served: impl AsRef<Path>, path: &str) -> Vec<Problem> {
    let served = ServedFolder::canonical(served.as_ref().to_path_buf());
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
        || devpage_design::resolve(&Disk(&served), path),
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

/// Whether the design at `path` can be drawn at all — in each theme, when
/// it is themed: a drawing too big to paint is the design's problem
/// everywhere — on its canvas, in its PNG and in `devpage check` — not only
/// where a picture is asked for.
fn drawable(renderer: &mut Renderer, path: &str, scene: &Scene) -> Result<(), Vec<Problem>> {
    let themes: &[Theme] = if scene.themed {
        &[Theme::Light, Theme::Dark]
    } else {
        &[Theme::Light]
    };
    guarded(
        path,
        || {
            themes
                .iter()
                .try_for_each(|&theme| renderer.drawing(scene, theme, None).map(|_| ()))
        },
        |p| devpage_design::render::Undrawable {
            message: p.message,
            at: None,
        },
    )
    .map_err(|undrawable| {
        // At the attribute that broke it, or else the design's start.
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
    })
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

    #[test]
    fn a_drawing_too_big_to_paint_is_a_problem_everywhere() {
        let dir = served("too-big");
        std::fs::write(
            dir.join("app/huge.design"),
            r##"<Frame width={20000} height={10} fill="#000000" />"##,
        )
        .unwrap();
        let expected = "app/huge.design:1:1: the drawing is 20000×10 pixels; a design draws at most 16384 a side";
        // In the store, which the canvas's stream and the PNG read…
        let designs = Designs::new(dir.clone());
        let found = designs.resolve("app/huge.design").unwrap_err();
        assert_eq!(found[0].to_string(), expected);
        // …and in what `devpage check` reads from disk.
        let found = problems(&dir, "/app/huge.design");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].to_string(), expected);
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
        let designs = Designs::new(dir.clone());
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
        // A saved image file is decoded anew.
        std::fs::write(dir.join("app/p.jpg"), &shirt).unwrap();
        designs.changed(&dir.join("app/p.jpg"));
        let third = designs.resolve("app/photo.design").unwrap();
        assert!(!Arc::ptr_eq(&file(&second), &file(&third)));
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
        let found = problems(&dir, "app/photo.design");
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].to_string().starts_with(expected), "{found:?}");
        // …and the PNG, instead of a blank picture.
        let designs = Designs::new(dir.clone());
        let png = designs.png("app/photo.design", Theme::Light).unwrap_err();
        assert!(png.starts_with(expected), "{png}");
    }

    #[test]
    fn only_the_designs_made_from_a_changed_file_are_announced() {
        let dir = served("store");
        let designs = Designs::new(dir.clone());
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
        let designs = Designs::new(dir.clone());
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
        let designs = Designs::new(dir.clone());
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
        let (_, w, h) = devpage_design::render::pixels(&png).unwrap();
        assert_eq!((w, h), (20, 20));
    }

    #[test]
    fn creating_a_missing_import_resolves_the_design_again() {
        let dir = served("create");
        let designs = Designs::new(dir.clone());
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
        let designs = Designs::new(dir.clone());
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
        let designs = Designs::new(dir.clone());
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
            super::problems(&dir, "/app/set.design")[0].to_string(),
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
        let designs = Designs::new(dir.join("app"));
        assert!(designs.resolve("../outside.design").is_err());
    }
}
