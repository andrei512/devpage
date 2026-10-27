//! The design store: every design asked for kept resolved in memory and
//! resolved again only when a save touched what it read, its problems and
//! its PNG. The watch stream and the canvas page that show designs in the
//! browser are the server's, in `server::watch_stream` and `server::canvas`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use devpage_design::resolve::ImageFile;
use devpage_design::{Files, Problem, Renderer, Scene, Theme};
use tokio::sync::broadcast;

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
/// outside the served folder.
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
}

struct Store {
    served: PathBuf,
    renderer: Mutex<Renderer>,
    sources: Mutex<HashMap<String, Source>>,
    /// How many files the store has read from disk.
    reads: std::sync::atomic::AtomicU64,
    /// Locked before `sources` whenever both are.
    resolved: Mutex<HashMap<String, Resolved>>,
    /// The address of each design whose scene or problems changed.
    updates: broadcast::Sender<String>,
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

impl Files for Reading<'_> {
    /// Any file's bytes — a design's text, or an image's — read once and
    /// kept until the watcher reports it changed.
    fn bytes(&self, path: &str) -> Option<Vec<u8>> {
        self.asked.borrow_mut().push(path.to_string());
        let mut sources = lock(&self.store.sources);
        if let Some(source) = sources.get(path) {
            self.seen.borrow_mut().push((path.to_string(), source.read));
            return source.bytes.clone();
        }
        let file = self
            .store
            .served
            .join(path)
            .canonicalize()
            .ok()
            .filter(|file| file.starts_with(&self.store.served) && file.is_file());
        let source = Source {
            bytes: file.as_ref().and_then(|file| std::fs::read(file).ok()),
            canonical: file.as_ref().map(|file| self.store.relative(file)),
            read: 1 + self
                .store
                .reads
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            image: None,
        };
        self.seen.borrow_mut().push((path.to_string(), source.read));
        let bytes = source.bytes.clone();
        sources.insert(path.to_string(), source);
        drop(sources);
        #[cfg(test)]
        if let Some(after) = lock(&self.store.after_read).as_ref() {
            after(path);
        }
        bytes
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
    fn relative(&self, file: &Path) -> String {
        file.strip_prefix(&self.served)
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
            served,
            renderer: Mutex::new(Renderer::new()),
            sources: Mutex::default(),
            reads: Default::default(),
            resolved: Mutex::default(),
            updates: broadcast::channel(64).0,
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

    /// The address of each design whose scene or problems change from now
    /// on, as the engine names it — what [`Designs::resolve`] then answers
    /// anew. A receiver that falls behind is told it lagged.
    pub fn changes(&self) -> broadcast::Receiver<String> {
        self.0.updates.subscribe()
    }

    /// A file in the served folder changed, was created or was deleted: forget
    /// it, resolve again every design that asked for it, and announce the
    /// designs whose scene or problems changed.
    pub fn changed(&self, file: &Path) {
        let store = &self.0;
        let relative = store.relative(file);
        // Forgotten and looked up at once, so a design is either kept already
        // and found here, or not yet kept and made again by `refresh`.
        let affected: Vec<(String, Resolved)> = {
            let resolved = lock(&store.resolved);
            let mut sources = lock(&store.sources);
            let forgotten = sources
                .iter()
                .filter(|(key, source)| {
                    **key == relative || source.canonical.as_deref() == Some(relative.as_str())
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
                let _ = store.updates.send(path);
            }
        }
    }

    /// The validator of the design at `path`'s PNG in `theme`, known before
    /// painting: a hash of its resolved scene — every node and the image
    /// files it shows — the theme, and devpage's build, which paints it.
    /// `None` when it doesn't resolve.
    pub async fn etag(&self, path: &str, theme: Theme) -> Option<String> {
        let designs = self.clone();
        let path = path.to_string();
        tokio::task::spawn_blocking(move || {
            let scene = designs.resolve(&path).ok()?;
            let mut text = format!("{}\0{theme:?}\0{:?}", crate::addresses::BUILD, scene.root);
            for id in scene.images.keys() {
                text.push_str(&format!("\0{id:?}"));
            }
            Some(crate::addresses::hash(text.as_bytes()))
        })
        .await
        .ok()
        .flatten()
    }

    /// The design at `path` painted as a PNG in `theme`, or its problems one
    /// per line.
    pub async fn png(&self, path: String, theme: Theme) -> Result<Vec<u8>, String> {
        let designs = self.clone();
        tokio::task::spawn_blocking(move || {
            let scene = designs
                .resolve(&path)
                .map_err(|problems| lines(&problems))?;
            let mut renderer = lock(&designs.0.renderer);
            guarded(
                &path,
                || devpage_design::png(&mut renderer, &scene, theme),
                |p| p.message,
            )
            .map_err(|e| format!("{}: {e}", address(&path)))
        })
        .await
        .expect("painting")
    }
}

/// The design at `path`'s problems, read straight from disk: what
/// `devpage check` and a page's problems bar report.
pub fn problems(served: &Path, path: &str) -> Vec<Problem> {
    struct Disk<'a>(&'a Path);
    impl Files for Disk<'_> {
        fn bytes(&self, path: &str) -> Option<Vec<u8>> {
            let file = self.0.join(path).canonicalize().ok()?;
            file.starts_with(self.0)
                .then(|| std::fs::read(file).ok())
                .flatten()
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::document::Document;
    use crate::index::Index;
    use crate::server::{Identity, Options, router};

    pub(crate) const BUTTON: &str = r##"<Frame props={{ label: "Button" }} layout="row" padding={[12, 20]} fill="#2563eb">
  <Text color="#ffffff">{label}</Text>
</Frame>
"##;
    pub(crate) const SCREEN: &str = r##"import Button from "./button.design"

<Frame width={200} height={120} layout="column" gap={8} padding={16} fill="#f8fafc">
  <Text>Cart</Text>
  <Button label="Pay" />
</Frame>
"##;
    pub(crate) const OTHER: &str = r##"<Frame width={50} height={50} fill="#000000" />
"##;

    pub(crate) fn served(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("devpage-designs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("app")).unwrap();
        std::fs::write(dir.join("app/button.design"), BUTTON).unwrap();
        std::fs::write(dir.join("app/screen.design"), SCREEN).unwrap();
        std::fs::write(dir.join("app/other.design"), OTHER).unwrap();
        dir.canonicalize().unwrap()
    }

    /// Writes a file and tells the store, as the watcher does.
    pub(crate) fn save(designs: &Designs, file: &Path, text: &str) {
        std::fs::write(file, text).unwrap();
        designs.changed(file);
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
            Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/designs/checkout/shirt.jpg"),
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

    #[tokio::test]
    async fn an_image_that_does_not_decode_is_a_problem_everywhere() {
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
        let png = designs
            .png("app/photo.design".into(), Theme::Light)
            .await
            .unwrap_err();
        assert!(png.starts_with(expected), "{png}");
    }

    #[test]
    fn only_the_designs_made_from_a_changed_file_are_announced() {
        let dir = served("store");
        let designs = Designs::new(dir.clone());
        let mut updates = designs.0.updates.subscribe();
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

    #[tokio::test]
    async fn no_design_file_stops_the_store() {
        let dir = served("hostile");
        let designs = Designs::new(dir.clone());
        let other = dir.join("app/other.design");
        assert!(
            designs
                .png("app/other.design".into(), Theme::Light)
                .await
                .is_ok()
        );
        // A colour with a pasted zero-width space, saved over a design held
        // in memory: what once panicked the watcher's thread.
        save(&designs, &other, &OTHER.replace("#000000", "#000\u{200b}"));
        let problems = designs
            .png("app/other.design".into(), Theme::Light)
            .await
            .unwrap_err();
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
                designs
                    .png("app/other.design".into(), Theme::Light)
                    .await
                    .is_err(),
                "{text:.40}"
            );
        }
        // And the store still sees saves afterwards.
        save(&designs, &other, &OTHER.replace("{50}", "{20}"));
        let png = designs
            .png("app/other.design".into(), Theme::Light)
            .await
            .unwrap();
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

    pub(crate) const THEMES: &str =
        "kind: themes\nlight:\n  bg: \"#ffffff\"\ndark:\n  bg: \"#000000\"\n";

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
        let mut updates = designs.0.updates.subscribe();
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

    #[test]
    fn a_design_outside_the_served_folder_is_never_read() {
        let dir = served("outside");
        std::fs::write(dir.join("../outside.design"), OTHER).unwrap();
        let designs = Designs::new(dir.join("app"));
        assert!(designs.resolve("../outside.design").is_err());
    }

    pub(crate) async fn start(dir: &Path, entry: &str) -> (String, Designs) {
        let document = Arc::new(Document::load(&dir.join(entry), dir).unwrap());
        let index = Arc::new(Index::build(dir));
        let designs = Designs::new(dir.to_path_buf());
        let changes = broadcast::channel(16).0;
        let router = router(Options {
            index,
            designs: designs.clone(),
            comments: Some(Identity::Local("T".into())),
            preview: crate::check::Preview::shared(),
            ..Options::new(document, changes)
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await.ok() });
        (address, designs)
    }

    #[tokio::test]
    async fn a_png_answers_304_from_its_scene_without_painting() {
        let dir = served("png-etag");
        let (address, designs) = start(&dir, "app/screen.design").await;
        let url = format!("{address}/app/screen.design.png");
        let client = reqwest::Client::new();
        let first = client.get(&url).send().await.unwrap();
        assert_eq!(first.status(), 200);
        assert_eq!(first.headers()["cache-control"], "no-cache");
        let tag = first.headers()["etag"].to_str().unwrap().to_string();
        let again = client
            .get(&url)
            .header("if-none-match", &tag)
            .send()
            .await
            .unwrap();
        assert_eq!(again.status(), 304);
        assert_eq!(again.headers()["etag"], tag.as_str());
        // A change to a design it uses is a new scene, and a new PNG.
        let button = std::fs::read_to_string(dir.join("app/button.design")).unwrap();
        save(
            &designs,
            &dir.join("app/button.design"),
            &button.replace("#2563eb", "#16a34a"),
        );
        let changed = client
            .get(&url)
            .header("if-none-match", &tag)
            .send()
            .await
            .unwrap();
        assert_eq!(changed.status(), 200);
        assert_ne!(changed.headers()["etag"], tag.as_str());
    }

    #[tokio::test]
    async fn a_png_or_the_problems() {
        let dir = served("png");
        let (address, _) = start(&dir, "app/screen.design").await;
        let response = reqwest::get(format!("{address}/app/screen.design.png"))
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["content-type"], "image/png");
        let png = response.bytes().await.unwrap();
        assert_eq!(&png[1..4], b"PNG");
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 200);

        std::fs::write(
            dir.join("app/broken.design"),
            "import Button from \"./button.design\"\n<Frame>\n  <Button lable=\"x\" />\n  <Text size=\"big\">a</Text>\n</Frame>\n",
        )
        .unwrap();
        let response = reqwest::get(format!("{address}/app/broken.design.png"))
            .await
            .unwrap();
        assert_eq!(response.status(), 422);
        let text = response.text().await.unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert!(lines[0].starts_with("app/broken.design:3:"), "{text}");
        assert!(lines[1].starts_with("app/broken.design:4:"), "{text}");
    }

    #[tokio::test]
    async fn a_png_in_either_theme() {
        let dir = served("png-theme");
        std::fs::write(dir.join("themes.yml"), THEMES).unwrap();
        std::fs::write(
            dir.join("app/themed.design"),
            "<Frame width={10} height={10} fill={theme.bg} />",
        )
        .unwrap();
        let (address, _) = start(&dir, "app/screen.design").await;
        let get = |query: &str| reqwest::get(format!("{address}/app/themed.design.png{query}"));
        let mut tags = Vec::new();
        for (query, value) in [("", 255), ("?theme=light", 255), ("?theme=dark", 0)] {
            let response = get(query).await.unwrap();
            assert_eq!(response.status(), 200, "{query}");
            tags.push(response.headers()["etag"].to_str().unwrap().to_string());
            let png = response.bytes().await.unwrap();
            let (pixels, _, _) = devpage_design::render::pixels(&png).unwrap();
            assert_eq!(pixels[..4], [value, value, value, 255], "{query}");
        }
        assert_eq!(tags[0], tags[1]);
        assert_ne!(tags[0], tags[2], "each theme is its own PNG");
        let wrong = get("?theme=blue").await.unwrap();
        assert_eq!(wrong.status(), 400);
        assert_eq!(
            wrong.text().await.unwrap(),
            "theme: expected light or dark, found \"blue\""
        );
    }

    #[tokio::test]
    async fn no_png_of_a_design_outside_the_served_folder() {
        let dir = served("png-outside");
        let outside = dir.join("../png-outside-secret.design");
        std::fs::write(&outside, "<Frame width={10} height={10} />").unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("app/escape.design")).unwrap();
        let (address, _) = start(&dir, "app/screen.design").await;
        for path in [
            "/../png-outside-secret.design.png",
            "/%2E%2E/png-outside-secret.design.png",
            "/app/escape.design.png",
            "/app/missing.design.png",
        ] {
            let response = reqwest::get(format!("{address}{path}")).await.unwrap();
            assert_eq!(response.status(), 404, "{path}");
            let body = response.text().await.unwrap();
            assert!(
                body.ends_with("is not part of the served folder"),
                "{path}: {body}"
            );
        }
    }

    #[tokio::test]
    async fn a_real_file_named_like_a_designs_png_wins() {
        let dir = served("png-real");
        std::fs::write(dir.join("app/screen.design.png"), b"not a drawing").unwrap();
        let (address, _) = start(&dir, "app/screen.design").await;
        let real = reqwest::get(format!("{address}/app/screen.design.png"))
            .await
            .unwrap();
        assert_eq!(real.status(), 200);
        assert_eq!(real.bytes().await.unwrap().as_ref(), b"not a drawing");
        // Without one, the address is the design's drawing.
        let drawn = reqwest::get(format!("{address}/app/other.design.png"))
            .await
            .unwrap();
        assert_eq!(drawn.headers()["content-type"], "image/png");
    }

    #[tokio::test]
    async fn a_designs_png_is_a_link_check_accepts() {
        let dir = served("png-link");
        std::fs::write(
            dir.join("app/broken.design"),
            SCREEN.replace("<Button label", "<Button lable"),
        )
        .unwrap();
        std::fs::write(
            dir.join("page.md"),
            "# Page\n\n![Screen](app/screen.design.png)\n\n![Broken](app/broken.design.png)\n\n![Nothing](app/nothing.design.png)\n",
        )
        .unwrap();
        let document = Document::load(&dir.join("page.md"), &dir).unwrap();
        let problems: Vec<String> = crate::check::document(&document, None)
            .await
            .iter()
            .map(|p| p.to_string())
            .collect();
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(
            problems[0].starts_with(
                "page.md:5: broken design app/broken.design.png — app/broken.design:5:"
            ),
            "{problems:?}"
        );
        assert_eq!(
            problems[1],
            "page.md:7: broken link app/nothing.design.png — no such file"
        );
    }

    #[tokio::test]
    async fn a_design_is_an_entry_document() {
        let dir = served("entry");
        let (address, _) = start(&dir, "app/screen.design").await;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let home = client.get(format!("{address}/")).send().await.unwrap();
        assert_eq!(home.status(), 307);
        assert_eq!(home.headers()["location"], "/app/screen.design");
        let page = client
            .get(format!("{address}/app/screen.design"))
            .send()
            .await
            .unwrap();
        assert_eq!(page.status(), 200);
        assert!(page.text().await.unwrap().contains("<canvas"));
    }

    #[tokio::test]
    async fn check_reports_a_broken_embedded_design_where_it_is_wrong() {
        let dir = served("check");
        std::fs::write(
            dir.join("app/screen.design"),
            SCREEN.replace("<Button label", "<Button lable"),
        )
        .unwrap();
        std::fs::write(
            dir.join("page.md"),
            "# Page\n\n![Screen](app/screen.design)\n\nEmbed with `![Name](name.design)`:\n\n```markdown\n![Gone](gone.design)\n```\n",
        )
        .unwrap();
        let document = Document::load(&dir.join("page.md"), &dir).unwrap();
        let problems = crate::check::document(&document, None).await;
        assert_eq!(problems.len(), 1, "{problems:?}");
        let shown = problems[0].to_string();
        assert!(
            shown.starts_with("page.md:3: broken design app/screen.design — app/screen.design:5:"),
            "{shown}"
        );
        assert!(shown.contains("lable"), "{shown}");

        let design = Document::load(&dir.join("app/screen.design"), &dir).unwrap();
        let problems = crate::check::document(&design, None).await;
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].to_string().starts_with("app/screen.design:5:"),
            "{problems:?}"
        );
    }
}

#[cfg(test)]
mod canvas_tests;
