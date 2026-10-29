//! A slide as an image: `<deck>.yml/<n>.png` is slide `n` of the deck, as the
//! browser devpage drives shows it on a 1920 × 1080 screen — so an agent
//! writing a slide's notes can look at the slide.
//!
//! One Chrome session draws every slide for a server: started on the first
//! shot, a tab per shot, two shots at a time, closed once idle. A slide is
//! shot without comments and without problems, so what it drew is kept until
//! a file its deck is drawn from changes, whatever is commented meanwhile,
//! whatever a link on it comes to reach, and whatever another deck's files
//! do.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use anyhow::Result;
use axum::Router;
use tokio::sync::Semaphore;

use crate::adapters::chrome::{Open, Profile, Session};

/// devpage's own controls, which are not part of the slide.
const HIDE_CONTROLS: &str = "document.head.insertAdjacentHTML('beforeend', \
    '<style>#devpage-corner, #devpage-comments-toggle { display: none !important; }</style>')";

/// Shots drawn at once; more wait their turn.
const AT_ONCE: u32 = 2;
/// How long Chrome stays open with nothing to draw.
const IDLE: Duration = Duration::from_secs(60);
/// How many bytes of PNGs the cache keeps; the oldest go first.
const CACHE_BYTES: usize = 64 * 1024 * 1024;

/// Slide `n` of a deck, as drawn.
#[derive(Clone)]
pub struct Shot {
    pub png: axum::body::Bytes,
    /// A validator for the picture, so a repeat request is a 304.
    pub etag: String,
    /// False when the slide wasn't ready after 20 s and was drawn as it was.
    pub ready: bool,
}

/// A server's slide PNGs: its Chrome session and the shots it drew. Cheap to
/// clone.
#[derive(Clone)]
pub struct Shots(Arc<Inner>);

struct Inner {
    session: tokio::sync::Mutex<Option<Arc<Session>>>,
    turns: Semaphore,
    cache: Mutex<Cache>,
    /// The shots being drawn, each told of changes as the kept ones are.
    drawing: Mutex<Vec<Weak<Drawing>>>,
    used: Mutex<Instant>,
}

/// What a slide's PNG is kept with: the picture and the files its deck was
/// drawn from.
struct Kept {
    shot: Shot,
    files: Vec<PathBuf>,
}

#[derive(Default)]
struct Cache {
    shots: HashMap<(String, usize), Kept>,
    order: VecDeque<(String, usize)>,
    bytes: usize,
}

/// A shot being drawn: the files its deck is drawn from, once they are
/// known, and whether one of them — or, before they are known, any file —
/// changed since it started, so its picture may be stale.
#[derive(Default)]
struct Drawing {
    files: Mutex<Option<Vec<PathBuf>>>,
    stale: AtomicBool,
}

impl Default for Shots {
    fn default() -> Self {
        Self::new()
    }
}

impl Shots {
    /// No Chrome yet and nothing drawn.
    pub fn new() -> Self {
        Self(Arc::new(Inner {
            session: tokio::sync::Mutex::new(None),
            turns: Semaphore::new(AT_ONCE as usize),
            cache: Mutex::new(Cache::default()),
            drawing: Mutex::new(Vec::new()),
            used: Mutex::new(Instant::now()),
        }))
    }

    /// Slide `n` of the deck served at `deck`, if it was drawn since a file
    /// its deck is drawn from last changed.
    pub fn cached(&self, deck: &str, n: usize) -> Option<Shot> {
        let cache = self.0.cache.lock().unwrap();
        cache
            .shots
            .get(&(deck.to_string(), n))
            .map(|kept| kept.shot.clone())
    }

    /// Slide `n` (1-based) of the deck served at `deck`, as a 1920 × 1080
    /// PNG: from the cache, or drawn by Chrome loading the deck from the
    /// router `serve` makes — started, like Chrome, on the first shot. That
    /// router serves the deck with comments and problems off. `drawn_from` names the
    /// files the deck is drawn from, read before the slide is drawn; the
    /// slide is kept until one of them changes.
    pub async fn png(
        &self,
        deck: &str,
        n: usize,
        drawn_from: impl FnOnce() -> Vec<PathBuf> + Send + 'static,
        serve: impl FnOnce() -> Router,
    ) -> Result<Shot> {
        if let Some(shot) = self.cached(deck, n) {
            return Ok(shot);
        }
        let _turn = self.0.turns.acquire().await?;
        // Heard of from now on, so a change while the files are read, or
        // while the slide is drawn, isn't missed.
        let drawing = Arc::new(Drawing::default());
        {
            let mut drawings = self.0.drawing.lock().unwrap();
            // Those drawn already go, even where no change is ever heard.
            drawings.retain(|drawn| drawn.strong_count() > 0);
            drawings.push(Arc::downgrade(&drawing));
        }
        // Reading a deck and resolving its designs block.
        let files = tokio::task::spawn_blocking(drawn_from).await?;
        *drawing.files.lock().unwrap() = Some(files.clone());
        let session = self.session(serve).await?;
        let drawn = capture(&session, &format!("{deck}#{n}")).await;
        *self.0.used.lock().unwrap() = Instant::now();
        let (png, ready) = drawn?;
        let shot = Shot {
            etag: etag(&png),
            png: png.into(),
            ready,
        };
        // Checked and kept under the cache's lock, which a change takes
        // first: a change either made it stale already, or finds it kept.
        let mut cache = self.0.cache.lock().unwrap();
        if !drawing.stale.load(Ordering::SeqCst) {
            cache.insert(
                (deck.to_string(), n),
                Kept {
                    shot: shot.clone(),
                    files,
                },
            );
        }
        Ok(shot)
    }

    /// Slide `n` of `deck` kept as drawn from `files`, without Chrome: for
    /// the tests.
    #[cfg(test)]
    pub(in crate::drive) fn keep(&self, deck: &str, n: usize, shot: Shot, files: Vec<PathBuf>) {
        self.0
            .cache
            .lock()
            .unwrap()
            .insert((deck.to_string(), n), Kept { shot, files });
    }

    /// The running session, started — Chrome with the router `serve` makes —
    /// if there is none or its Chrome has stopped.
    async fn session(&self, serve: impl FnOnce() -> Router) -> Result<Arc<Session>> {
        let mut session = self.0.session.lock().await;
        if let Some(running) = session.as_ref()
            && running.running()
        {
            return Ok(running.clone());
        }
        if let Some(stopped) = session.take() {
            close(stopped).await;
        }
        let started = Arc::new(Session::start(Profile::Shot, serve()).await?);
        *session = Some(started.clone());
        *self.0.used.lock().unwrap() = Instant::now();
        tokio::spawn(close_when_idle(Arc::downgrade(&self.0)));
        Ok(started)
    }

    /// Closes Chrome, once the shots it is drawing are done.
    pub async fn close(&self) {
        self.0.close(false).await;
    }

    /// Whether Chrome is running: for the tests.
    #[cfg(test)]
    pub(in crate::drive) async fn running(&self) -> bool {
        self.0.session.lock().await.is_some()
    }
}

/// The watcher's word that files changed: every kept slide whose deck is
/// drawn from one of them is forgotten, and every slide being drawn from
/// one — or whose files aren't known yet — won't be kept. A comment changes
/// no file of a deck: no slide is shot with its comments. Nor does a file a
/// slide only links to, whose arrival fixes a broken link: no slide is shot
/// with its problems.
impl devpage_core::watch::SlidePngs for Shots {
    fn changed(&self, files: &[PathBuf]) {
        let mut cache = self.0.cache.lock().unwrap();
        let gone: Vec<(String, usize)> = cache
            .shots
            .iter()
            .filter(|(_, kept)| reaches(files, &kept.files))
            .map(|(key, _)| key.clone())
            .collect();
        for key in gone {
            cache.remove(&key);
        }
        self.0.drawing.lock().unwrap().retain(|drawing| {
            let Some(drawing) = drawing.upgrade() else {
                return false;
            };
            let stale = match &*drawing.files.lock().unwrap() {
                Some(drawn_from) => reaches(files, drawn_from),
                None => true,
            };
            if stale {
                drawing.stale.store(true, Ordering::SeqCst);
            }
            true
        });
    }
}

/// Whether a change to `changed` reaches what was drawn from `drawn_from`:
/// one of them changed, a file inside one of them — a widgets folder — or a
/// folder holding one.
fn reaches(changed: &[PathBuf], drawn_from: &[PathBuf]) -> bool {
    changed.iter().any(|changed| {
        drawn_from
            .iter()
            .any(|file| changed.starts_with(file) || file.starts_with(changed))
    })
}

impl Inner {
    /// Closes Chrome — when `idle`, only if it has drawn nothing for
    /// [`IDLE`]. It takes every turn first, so no shot is cut short and none
    /// starts until Chrome is closed.
    async fn close(&self, idle: bool) {
        let Ok(_turns) = self.turns.acquire_many(AT_ONCE).await else {
            return;
        };
        if idle && self.used.lock().unwrap().elapsed() < IDLE {
            return;
        }
        if let Some(running) = self.session.lock().await.take() {
            close(running).await;
        }
    }
}

/// Closes `session` if no shot holds it still; otherwise the last shot to
/// let it go drops it, which kills its Chrome.
async fn close(session: Arc<Session>) {
    if let Ok(session) = Arc::try_unwrap(session) {
        session.close().await;
    }
}

impl Cache {
    fn insert(&mut self, key: (String, usize), kept: Kept) {
        self.remove(&key);
        self.bytes += kept.shot.png.len();
        self.shots.insert(key.clone(), kept);
        self.order.push_back(key);
        while self.bytes > CACHE_BYTES
            && let Some(oldest) = self.order.pop_front()
        {
            if let Some(old) = self.shots.remove(&oldest) {
                self.bytes -= old.shot.png.len();
            }
        }
    }

    fn remove(&mut self, key: &(String, usize)) {
        if let Some(old) = self.shots.remove(key) {
            self.bytes -= old.shot.png.len();
            self.order.retain(|k| k != key);
        }
    }
}

/// Closes Chrome once it has drawn nothing for [`IDLE`], unless the server's
/// shots are gone first.
async fn close_when_idle(inner: Weak<Inner>) {
    loop {
        let left = {
            let Some(inner) = inner.upgrade() else { return };
            if inner.session.lock().await.is_none() {
                return;
            }
            let idle = inner.used.lock().unwrap().elapsed();
            let busy = inner.turns.available_permits() < AT_ONCE as usize;
            if idle >= IDLE && !busy {
                inner.close(true).await;
                continue;
            }
            IDLE.saturating_sub(idle).max(Duration::from_secs(1))
        };
        tokio::time::sleep(left).await;
    }
}

/// The picture's validator: a hash of its bytes.
fn etag(png: &[u8]) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    png.hash(&mut hasher);
    format!("\"{:016x}\"", hasher.finish())
}

/// Opens `path` in a new tab, shot, on a 1920 × 1080 screen, once the page is
/// ready, and screenshots it without devpage's controls; the tab is closed
/// after. False beside the picture when the page was never ready.
async fn capture(session: &Session, path: &str) -> Result<(Vec<u8>, bool)> {
    let tab = session
        .open(
            path,
            Open {
                shot: true,
                screen: Some((1920, 1080)),
                ..Open::default()
            },
        )
        .await?;
    let shot = async {
        tab.eval::<()>(HIDE_CONTROLS).await?;
        anyhow::Ok((tab.screenshot().await?, tab.ready()))
    }
    .await;
    tab.close().await;
    shot
}

#[cfg(test)]
mod tests {
    use super::*;
    use devpage_core::designs::Designs;
    use devpage_core::document::Document;
    use devpage_core::folder::{Folder, ServedFolder};
    use devpage_core::watch::Listeners;
    use devpage_core::widgets::Modules;

    use crate::adapters::changes::Changes;

    fn shot() -> Shot {
        Shot {
            png: axum::body::Bytes::from_static(b"png"),
            etag: etag(b"png"),
            ready: true,
        }
    }

    /// A served folder of two decks: `a.yml`, whose slide embeds an image
    /// and a design that shows another image and sets text in a font from
    /// the folder `fonts/Body/`, and widgets from `widgets/`;
    /// and `b.yml`, a slide of its own. The watcher's listeners on it, telling
    /// `shots`, and its slides kept as drawn — every file but those in
    /// `without`, which aren't there yet.
    fn two_decks(name: &str, shots: &Shots) -> (PathBuf, Listeners) {
        two_decks_without(name, shots, &[])
    }

    fn two_decks_without(name: &str, shots: &Shots, without: &[&str]) -> (PathBuf, Listeners) {
        let dir = std::env::temp_dir().join(format!("devpage-shots-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("widgets")).unwrap();
        let dir = dir.canonicalize().unwrap();
        let write = |file: &str, text: &str| {
            if !without.contains(&file) {
                std::fs::write(dir.join(file), text).unwrap();
            }
        };
        write(
            "a.md",
            "# A\n\n![Logo](logo.png)\n\n![Screen](screen.design)\n\n[B](b.md)\n",
        );
        write("logo.png", "png");
        write("photo.png", "png");
        std::fs::create_dir_all(dir.join("fonts/Body")).unwrap();
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("design/fonts/Inter-Regular.ttf"),
            dir.join("fonts/Body/Body-Regular.ttf"),
        )
        .unwrap();
        write(
            "themes.yml",
            "kind: themes\nfonts:\n  body: ./fonts/Body\nlight: {}\ndark: {}\n",
        );
        write(
            "screen.design",
            "<Frame width={10} height={10}><Image src=\"photo.png\" width={10} height={10} /><Text font=\"body\">Hi</Text></Frame>\n",
        );
        write(
            "widgets/Badge.tsx",
            "export default function Badge() { return null; }\n",
        );
        write(
            "a.yml",
            "kind: slides\ntitle: A\nslides: [a.md]\nwidgets: widgets\n",
        );
        write("b.md", "# B\n");
        write("b.yml", "kind: slides\ntitle: B\nslides: [b.md]\n");
        let served = ServedFolder::new(&dir).unwrap();
        let entry = Document::load(&dir.join("a.yml"), &served).unwrap();
        let folder = Arc::new(Folder::new(Arc::new(entry)));
        let designs = Designs::new(&served);
        let modules = Modules::default();
        for deck in ["a.yml", "b.yml"] {
            let files =
                devpage_core::check::drawn_from(&dir.join(deck), &served, &designs, &modules);
            shots.keep(&format!("/{deck}"), 1, shot(), files);
        }
        let listeners = Listeners::new(
            folder,
            designs,
            modules,
            None,
            Arc::new(Changes::default()),
            None,
        )
        .with_slide_pngs(Arc::new(shots.clone()));
        (dir, listeners)
    }

    #[test]
    fn saving_a_file_of_one_deck_keeps_another_decks_slides() {
        for file in ["a.md", "a.yml"] {
            let shots = Shots::new();
            let (dir, listeners) = two_decks("own", &shots);
            listeners.changed(&[dir.join(file)]);
            assert!(shots.cached("/a.yml", 1).is_none(), "{file}");
            assert!(shots.cached("/b.yml", 1).is_some(), "{file}");
        }
    }

    #[test]
    fn saving_a_file_a_deck_embeds_forgets_its_slides() {
        for file in [
            "logo.png",
            "screen.design",
            "photo.png",
            "widgets/Badge.tsx",
            "widgets/New.tsx",
        ] {
            let shots = Shots::new();
            let (dir, listeners) = two_decks("embeds", &shots);
            listeners.changed(&[dir.join(file)]);
            assert!(shots.cached("/a.yml", 1).is_none(), "{file}");
            assert!(shots.cached("/b.yml", 1).is_some(), "{file}");
        }
    }

    /// An image a design on a slide shows, added after the slide was drawn
    /// without it, forgets that deck's slides.
    #[test]
    fn adding_an_image_an_embedded_design_lacks_forgets_its_slides() {
        let shots = Shots::new();
        let (dir, listeners) = two_decks_without("design-lacks", &shots, &["photo.png"]);
        std::fs::write(dir.join("photo.png"), "png").unwrap();
        listeners.changed(&[dir.join("photo.png")]);
        assert!(shots.cached("/a.yml", 1).is_none());
        assert!(shots.cached("/b.yml", 1).is_some());
    }

    /// A font added to the folder an embedded design's font is read from
    /// forgets that deck's slides, and no other deck's.
    #[test]
    fn a_font_added_to_an_embedded_designs_font_folder_forgets_its_slides() {
        let shots = Shots::new();
        let (dir, listeners) = two_decks("font-folder", &shots);
        let bold = dir.join("fonts/Body/Body-Bold.ttf");
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("design/fonts/Inter-Bold.ttf"),
            &bold,
        )
        .unwrap();
        listeners.changed(&[bold]);
        assert!(shots.cached("/a.yml", 1).is_none());
        assert!(shots.cached("/b.yml", 1).is_some());
    }

    #[test]
    fn a_page_a_slide_only_links_to_is_not_what_it_is_drawn_from() {
        let shots = Shots::new();
        let (dir, listeners) = two_decks("linked", &shots);
        listeners.changed(&[dir.join("b.md")]);
        assert!(shots.cached("/a.yml", 1).is_some());
        assert!(shots.cached("/b.yml", 1).is_none());
    }

    #[test]
    fn a_slide_being_drawn_when_its_files_change_is_not_kept() {
        use devpage_core::watch::SlidePngs;
        let shots = Shots::new();
        let unknown = Arc::new(Drawing::default());
        let other = Arc::new(Drawing::default());
        let own = Arc::new(Drawing::default());
        *other.files.lock().unwrap() = Some(vec![PathBuf::from("/x/b.md")]);
        *own.files.lock().unwrap() = Some(vec![PathBuf::from("/x/a.md")]);
        for drawing in [&unknown, &other, &own] {
            shots
                .0
                .drawing
                .lock()
                .unwrap()
                .push(Arc::downgrade(drawing));
        }
        shots.changed(&[PathBuf::from("/x/a.md")]);
        assert!(unknown.stale.load(Ordering::SeqCst));
        assert!(!other.stale.load(Ordering::SeqCst));
        assert!(own.stale.load(Ordering::SeqCst));
    }
}
