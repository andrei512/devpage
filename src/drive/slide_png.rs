//! A slide as an image: `<deck>.yml/<n>.png` is slide `n` of the deck, as the
//! browser devpage drives shows it on a 1920 × 1080 screen — so an agent
//! writing a slide's notes can look at the slide.
//!
//! One Chrome session draws every slide for a server: started on the first
//! shot, a tab per shot, two shots at a time, closed once idle. A slide is
//! shot without comments, so what it drew is kept until a watched file
//! changes, whatever is commented meanwhile.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use anyhow::Result;
use axum::Router;
use tokio::sync::Semaphore;

use crate::changes::{Changes, Subscription};
use crate::chrome::{Open, Profile, Session};

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
    /// The server's changes, read on each use: a refresh clears the cache.
    changes: Mutex<Subscription>,
    used: Mutex<Instant>,
}

#[derive(Default)]
struct Cache {
    shots: HashMap<(String, usize), Shot>,
    order: VecDeque<(String, usize)>,
    bytes: usize,
    /// Bumped each time a change clears the cache.
    generation: u64,
}

impl Shots {
    /// No Chrome yet and nothing drawn; a refresh on `changes` clears what
    /// will be.
    pub fn new(changes: &Changes) -> Self {
        Self(Arc::new(Inner {
            session: tokio::sync::Mutex::new(None),
            turns: Semaphore::new(AT_ONCE as usize),
            cache: Mutex::new(Cache::default()),
            changes: Mutex::new(changes.subscribe()),
            used: Mutex::new(Instant::now()),
        }))
    }

    /// Slide `n` of the deck served at `deck`, if it was drawn since the last
    /// change.
    pub fn cached(&self, deck: &str, n: usize) -> Option<Shot> {
        self.0.generation();
        let cache = self.0.cache.lock().unwrap();
        cache.shots.get(&(deck.to_string(), n)).cloned()
    }

    /// Slide `n` (1-based) of the deck served at `deck`, as a 1920 × 1080
    /// PNG: from the cache, or drawn by Chrome loading the deck from the
    /// router `serve` makes — started, like Chrome, on the first shot. That
    /// router serves the deck with comments off.
    pub async fn png(&self, deck: &str, n: usize, serve: impl FnOnce() -> Router) -> Result<Shot> {
        if let Some(shot) = self.cached(deck, n) {
            return Ok(shot);
        }
        let _turn = self.0.turns.acquire().await?;
        let generation = self.0.generation();
        let session = self.session(serve).await?;
        let drawn = capture(&session, &format!("{deck}#{n}")).await;
        *self.0.used.lock().unwrap() = Instant::now();
        let (png, ready) = drawn?;
        let shot = Shot {
            etag: etag(&png),
            png: png.into(),
            ready,
        };
        // A file changed while it was drawn: the picture may be stale, so it
        // isn't kept.
        if self.0.generation() == generation {
            self.0
                .cache
                .lock()
                .unwrap()
                .insert((deck.to_string(), n), shot.clone());
        }
        Ok(shot)
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
    async fn running(&self) -> bool {
        self.0.session.lock().await.is_some()
    }
}

impl Inner {
    /// The cache's generation, after clearing it if a refresh arrived since
    /// the last look. A comment change doesn't clear it: no slide is shot
    /// with its comments.
    fn generation(&self) -> u64 {
        let refreshed = self.changes.lock().unwrap().refreshed();
        let mut cache = self.cache.lock().unwrap();
        if refreshed {
            *cache = Cache {
                generation: cache.generation + 1,
                ..Cache::default()
            };
        }
        cache.generation
    }

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
    fn insert(&mut self, key: (String, usize), shot: Shot) {
        if let Some(old) = self.shots.remove(&key) {
            self.bytes -= old.png.len();
            self.order.retain(|k| k != &key);
        }
        self.bytes += shot.png.len();
        self.shots.insert(key.clone(), shot);
        self.order.push_back(key);
        while self.bytes > CACHE_BYTES
            && let Some(oldest) = self.order.pop_front()
        {
            if let Some(old) = self.shots.remove(&oldest) {
                self.bytes -= old.png.len();
            }
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
    use crate::changes::Change;
    use crate::document::Document;
    use crate::served::ServedFolder;
    use crate::server;

    /// A design that paints.
    const BOX: &str = "<Frame width={400} height={300} fill=\"#2563eb\" />";

    /// A folder of `files`, served with `talk.yml` as its entry; its address
    /// and the channel that tells the server a file changed.
    async fn deck(name: &str, files: &[(&str, &str)]) -> (String, Changes) {
        let dir = std::env::temp_dir().join(format!("devpage-shots-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (file, text) in files {
            std::fs::write(dir.join(file), text).unwrap();
        }
        let dir = dir.canonicalize().unwrap();
        let document = Arc::new(
            Document::load(&dir.join("talk.yml"), &ServedFolder::new(&dir).unwrap()).unwrap(),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let changes = crate::changes::Changes::default();
        let router = server::router(server::Options {
            comments: Some(crate::comments::Identity::Local("T".into())),
            ..server::Options::new(document, changes.clone())
        });
        tokio::spawn(async move { axum::serve(listener, router).await.ok() });
        (format!("http://{address}"), changes)
    }

    #[test]
    fn a_refresh_clears_the_kept_slides_and_a_comment_does_not() {
        let changes = Changes::default();
        let shots = Shots::new(&changes);
        let drawn = Shot {
            png: axum::body::Bytes::from_static(b"png"),
            etag: etag(b"png"),
            ready: true,
        };
        shots
            .0
            .cache
            .lock()
            .unwrap()
            .insert(("/talk.yml".into(), 1), drawn);
        changes.publish(Change::Comments("/one.md".into()));
        assert!(shots.cached("/talk.yml", 1).is_some());
        changes.publish(Change::Refresh);
        assert!(shots.cached("/talk.yml", 1).is_none());
    }

    /// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn a_slide_is_shot_without_its_comments() {
        let (address, changes) = deck(
            "comments",
            &[
                ("one.md", "# One\n\nThe client retries every write.\n"),
                ("talk.yml", "kind: slides\ntitle: Talk\nslides: [one.md]\n"),
            ],
        )
        .await;
        let url = format!("{address}/talk.yml/1.png");
        let before = reqwest::get(&url).await.unwrap().bytes().await.unwrap();
        let client = reqwest::Client::new();
        let made = client
            .post(format!("{address}/__devpage/comments"))
            .json(&serde_json::json!({
                "page": "/one.md",
                "anchor": { "quote": "retries", "prefix": "The client ", "suffix": " every write." },
                "text": "Bounded?",
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(made.status(), 200);
        // Drawn again, as it is now: the thread is not on it.
        changes.publish(Change::Refresh);
        let after = reqwest::get(&url).await.unwrap().bytes().await.unwrap();
        assert!(after == before, "the comment shows in the slide's PNG");
    }

    /// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn a_slide_beside_a_design_is_drawn_without_waiting_for_it() {
        let (address, _changes) = deck(
            "beside",
            &[
                ("box.design", BOX),
                ("one.md", "# One\n"),
                ("two.md", "# Two\n\n![The box](box.design)\n"),
                (
                    "talk.yml",
                    "kind: slides\ntitle: Talk\nslides: [one.md, two.md]\n",
                ),
            ],
        )
        .await;
        // The design on slide 2 is hidden on slide 1: not waited for.
        let started = Instant::now();
        let first = reqwest::get(format!("{address}/talk.yml/1.png"))
            .await
            .unwrap();
        assert_eq!(first.status(), 200);
        assert!(first.headers().get("x-devpage-ready").is_none());
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
        // On its own slide it is painted before the shot.
        let second = reqwest::get(format!("{address}/talk.yml/2.png"))
            .await
            .unwrap();
        assert_eq!(second.status(), 200);
        assert!(second.headers().get("x-devpage-ready").is_none());
    }

    /// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn a_slide_showing_its_own_png_returns_promptly() {
        let (address, _changes) = deck(
            "itself",
            &[
                ("one.md", "# One\n\n![Itself](talk.yml/1.png)\n"),
                ("talk.yml", "kind: slides\ntitle: Talk\nslides: [one.md]\n"),
            ],
        )
        .await;
        let started = Instant::now();
        let response = reqwest::get(format!("{address}/talk.yml/1.png"))
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    /// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn a_repeat_request_comes_from_the_cache_until_a_file_changes() {
        let (address, changes) = deck(
            "cache",
            &[
                ("one.md", "# One\n"),
                ("talk.yml", "kind: slides\ntitle: Talk\nslides: [one.md]\n"),
            ],
        )
        .await;
        let url = format!("{address}/talk.yml/1.png");
        let client = reqwest::Client::new();
        let shot = |held: Option<String>, inside: bool| {
            let mut request = client.get(&url);
            if let Some(etag) = held {
                request = request.header("if-none-match", etag);
            }
            if inside {
                request = request.header(crate::chrome::SHOT_HEADER, "1");
            }
            request.send()
        };
        // Inside another shot, a slide not yet drawn isn't drawn.
        assert_eq!(shot(None, true).await.unwrap().status(), 508);

        let first = shot(None, false).await.unwrap();
        let etag = first.headers()["etag"].to_str().unwrap().to_string();
        let first = first.bytes().await.unwrap();

        // Drawn already: no Chrome, and nothing sent to a holder of it.
        let started = Instant::now();
        let again = shot(Some(etag.clone()), false).await.unwrap();
        assert_eq!(again.status(), 304);
        assert_eq!(again.headers()["etag"], etag.as_str());
        let cached = shot(None, true).await.unwrap();
        assert_eq!(cached.status(), 200);
        assert_eq!(cached.bytes().await.unwrap(), first);
        assert!(
            started.elapsed() < Duration::from_millis(200),
            "{:?}",
            started.elapsed()
        );

        // A change: the slide is drawn again, as it is now.
        let dir = std::env::temp_dir()
            .join(format!("devpage-shots-{}-cache", std::process::id()))
            .canonicalize()
            .unwrap();
        std::fs::write(dir.join("one.md"), "# Changed\n").unwrap();
        changes.publish(Change::Refresh);
        let changed = shot(Some(etag.clone()), false).await.unwrap();
        assert_eq!(changed.status(), 200);
        assert_ne!(changed.headers()["etag"], etag.as_str());
        assert_ne!(changed.bytes().await.unwrap(), first);
    }

    /// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn chrome_closes_when_told_and_starts_again_for_the_next_shot() {
        let changes = crate::changes::Changes::default();
        let shots = Shots::new(&changes);
        let dir = std::env::temp_dir().join(format!("devpage-shots-{}-close", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("one.md"), "# One\n").unwrap();
        std::fs::write(
            dir.join("talk.yml"),
            "kind: slides\ntitle: Talk\nslides: [one.md]\n",
        )
        .unwrap();
        let dir = dir.canonicalize().unwrap();
        let document = Arc::new(
            Document::load(&dir.join("talk.yml"), &ServedFolder::new(&dir).unwrap()).unwrap(),
        );
        let serve = || server::router(server::Options::printing(document.clone(), changes.clone()));
        shots.png("/talk.yml", 1, serve).await.unwrap();
        assert!(shots.running().await);
        shots.close().await;
        assert!(!shots.running().await);
        changes.publish(Change::Refresh);
        shots.png("/talk.yml", 1, serve).await.unwrap();
        assert!(shots.running().await);
        shots.close().await;
    }

    /// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn a_slide_is_served_as_a_1920_by_1080_png() {
        let dir = std::env::temp_dir().join(format!("devpage-slide-png-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("one.md"), "# One\n").unwrap();
        std::fs::write(dir.join("two.md"), "# Two\n\n<Notes>Never drawn.</Notes>\n").unwrap();
        std::fs::write(
            dir.join("talk.yml"),
            "kind: slides\ntitle: Talk\nslides: [one.md, two.md]\n",
        )
        .unwrap();
        let dir = dir.canonicalize().unwrap();
        let document = Arc::new(
            Document::load(&dir.join("talk.yml"), &ServedFolder::new(&dir).unwrap()).unwrap(),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let changes = crate::changes::Changes::default();
        let router = server::router(server::Options {
            comments: Some(crate::comments::Identity::Local("T".into())),
            ..server::Options::new(document, changes)
        });
        tokio::spawn(async move { axum::serve(listener, router).await.ok() });

        let response = reqwest::get(format!("http://{address}/talk.yml/2.png"))
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["content-type"], "image/png");
        let png = response.bytes().await.unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        // The IHDR chunk's width and height, big-endian.
        let size = |at: usize| u32::from_be_bytes(png[at..at + 4].try_into().unwrap());
        assert_eq!((size(16), size(20)), (1920, 1080));

        // Slide 1 and slide 2 are different pictures.
        let first = reqwest::get(format!("http://{address}/talk.yml/1.png"))
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        assert_ne!(first, png);

        for path in ["talk.yml/3.png", "talk.yml/0.png", "one.md/1.png"] {
            let status = reqwest::get(format!("http://{address}/{path}"))
                .await
                .unwrap()
                .status();
            assert_eq!(status, 404, "{path}");
        }
    }
}
