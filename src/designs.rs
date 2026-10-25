//! Designs in the server: the store that keeps every design asked for
//! resolved in memory and resolves again only what a save touched, the watch
//! stream that carries a design's changes to the browser, and its PNG.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};

use devpage_design::proto::{Patch, Problems, Update, WatchRequest, update::Body};
use devpage_design::resolve::Node;
use devpage_design::{Files, Problem, Renderer, Scene, wire};
use tokio::sync::broadcast;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};

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
    /// Its canonical path from the top of the served folder, which is what the
    /// watcher reports.
    canonical: Option<String>,
}

struct Store {
    served: PathBuf,
    renderer: Mutex<Renderer>,
    sources: Mutex<HashMap<String, Source>>,
    resolved: Mutex<HashMap<String, Resolved>>,
    /// The address of each design whose scene or problems changed.
    updates: broadcast::Sender<String>,
    /// How many times a design was resolved, for the tests.
    #[cfg(test)]
    resolutions: std::sync::atomic::AtomicUsize,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The store's files, recording every path a resolving asks for.
struct Reading<'a> {
    store: &'a Store,
    asked: RefCell<Vec<String>>,
}

impl Files for Reading<'_> {
    /// Any file's bytes — a design's text, or an image's — read once and
    /// kept until the watcher reports it changed.
    fn bytes(&self, path: &str) -> Option<Vec<u8>> {
        self.asked.borrow_mut().push(path.to_string());
        let mut sources = lock(&self.store.sources);
        if let Some(source) = sources.get(path) {
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
        };
        let bytes = source.bytes.clone();
        sources.insert(path.to_string(), source);
        bytes
    }
}

impl Store {
    fn relative(&self, file: &Path) -> String {
        file.strip_prefix(&self.served)
            .unwrap_or(file)
            .to_string_lossy()
            .into_owned()
    }

    fn resolve(&self, path: &str) -> Resolved {
        #[cfg(test)]
        self.resolutions
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let reading = Reading {
            store: self,
            asked: RefCell::new(Vec::new()),
        };
        let result = guarded(
            path,
            || devpage_design::resolve(&reading, path),
            |p| vec![p],
        );
        let missing = lock(&self.sources)
            .get(path)
            .is_none_or(|source| source.bytes.is_none());
        Resolved {
            result,
            asked: reading.asked.into_inner(),
            missing,
        }
    }

    /// The design at `path`, from memory, resolving it the first time.
    fn get(&self, path: &str) -> Resolved {
        if let Some(resolved) = lock(&self.resolved).get(path) {
            return resolved.clone();
        }
        let resolved = self.resolve(path);
        lock(&self.resolved).insert(path.to_string(), resolved.clone());
        resolved
    }
}

/// Every design the server has been asked for, kept resolved in memory, and
/// the renderer that paints them: its fonts load once.
#[derive(Clone)]
pub struct Designs(Arc<Store>);

/// A design's address as the engine names files: from the top of the served
/// folder, without a leading slash.
fn address(path: &str) -> &str {
    path.trim_start_matches('/')
}

impl Designs {
    /// The store for the served folder, which must be canonical.
    pub fn new(served: PathBuf) -> Designs {
        Designs(Arc::new(Store {
            served,
            renderer: Mutex::new(Renderer::new()),
            sources: Mutex::default(),
            resolved: Mutex::default(),
            updates: broadcast::channel(64).0,
            #[cfg(test)]
            resolutions: Default::default(),
        }))
    }

    /// The design at `path` resolved, or its problems one per line.
    pub fn resolve(&self, path: &str) -> Result<Scene, Vec<Problem>> {
        self.0.get(address(path)).result
    }

    /// A file in the served folder changed, was created or was deleted: forget
    /// it, resolve again every design that asked for it, and announce the
    /// designs whose scene or problems changed.
    pub fn changed(&self, file: &Path) {
        let store = &self.0;
        let relative = store.relative(file);
        let forgotten: Vec<String> = {
            let mut sources = lock(&store.sources);
            let keys = sources
                .iter()
                .filter(|(key, source)| {
                    **key == relative || source.canonical.as_deref() == Some(relative.as_str())
                })
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            for key in &keys {
                sources.remove(key);
            }
            keys
        };
        if forgotten.is_empty() {
            return;
        }
        let affected: Vec<(String, Resolved)> = lock(&store.resolved)
            .iter()
            .filter(|(_, resolved)| resolved.asked.iter().any(|a| forgotten.contains(a)))
            .map(|(path, resolved)| (path.clone(), resolved.clone()))
            .collect();
        for (path, old) in affected {
            let new = store.resolve(&path);
            let differs = new.result != old.result || new.missing != old.missing;
            lock(&store.resolved).insert(path.clone(), new);
            if differs {
                let _ = store.updates.send(path);
            }
        }
    }

    /// The design at `path` painted as a PNG, or its problems one per line.
    pub async fn png(&self, path: String) -> Result<Vec<u8>, String> {
        let designs = self.clone();
        tokio::task::spawn_blocking(move || {
            let scene = designs
                .resolve(&path)
                .map_err(|problems| lines(&problems))?;
            let mut renderer = lock(&designs.0.renderer);
            guarded(
                &path,
                || devpage_design::png(&mut renderer, &scene),
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
    .err()
    .unwrap_or_default()
}

/// What `f` gives, or — should the engine ever panic on a design — a problem
/// saying so, so that no design file can take the server or its watcher down.
fn guarded<T, E>(
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

/// The watch stream: one per page, over every design the page shows. For
/// each, the scene, then a patch after each change that altered it, or its
/// problems; a deleted design's `no such file` is the last update about it,
/// and the stream ends when no design is left.
pub struct Watch {
    pub designs: Designs,
}

/// What one design's watcher last sent.
#[derive(Default)]
struct Sent {
    scene: Option<Node>,
    problems: Option<Vec<Problem>>,
}

impl Sent {
    /// The update `resolved` calls for after what was sent, if any.
    fn next(&mut self, resolved: Resolved) -> Option<Body> {
        match resolved.result {
            Ok(scene) => {
                // Problems on show: even an empty patch says the design is
                // valid again.
                let fixed = self.problems.take().is_some();
                let body = match &self.scene {
                    None => Some(Body::Scene(wire::encode(&scene.root))),
                    Some(old) => {
                        let replace = wire::diff(old, &scene.root);
                        (fixed || !replace.is_empty()).then_some(Body::Patch(Patch { replace }))
                    }
                };
                self.scene = Some(scene.root);
                body
            }
            Err(problems) if self.problems.as_ref() != Some(&problems) => {
                self.problems = Some(problems.clone());
                Some(Body::Problems(Problems {
                    problems: problems.iter().map(Problem::to_string).collect(),
                }))
            }
            Err(_) => None,
        }
    }
}

#[tonic::async_trait]
impl devpage_design::proto::designs_server::Designs for Watch {
    type WatchStream = Pin<Box<dyn Stream<Item = Result<Update, Status>> + Send>>;

    async fn watch(
        &self,
        request: Request<WatchRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        // Each design once, keyed by its address, answered with the name the
        // request gave it.
        let mut watched: Vec<(String, String, Sent)> = Vec::new();
        for named in request.into_inner().paths {
            let path = address(&named).to_string();
            if !watched.iter().any(|(p, _, _)| *p == path) {
                watched.push((path, named, Sent::default()));
            }
        }
        let store = self.designs.0.clone();
        let mut updates = store.updates.subscribe();
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        tokio::spawn(async move {
            let mut due: Vec<String> = watched.iter().map(|(p, _, _)| p.clone()).collect();
            loop {
                for path in due.drain(..) {
                    let Some(index) = watched.iter().position(|(p, _, _)| *p == path) else {
                        continue;
                    };
                    let resolved = store.get(&path);
                    let gone = resolved.missing;
                    let (_, named, sent) = &mut watched[index];
                    if let Some(body) = sent.next(resolved) {
                        let update = Update {
                            body: Some(body),
                            path: named.clone(),
                            gone,
                        };
                        if tx.send(Ok(update)).await.is_err() {
                            return;
                        }
                    }
                    if gone {
                        watched.remove(index);
                    }
                }
                if watched.is_empty() {
                    return;
                }
                // Wait for a watched design to change; falling behind means
                // look at every one again.
                match updates.recv().await {
                    Ok(changed) => due.push(changed),
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        due = watched.iter().map(|(p, _, _)| p.clone()).collect();
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
        Ok(Response::new(Box::pin(
            tokio_stream::wrappers::ReceiverStream::new(rx),
        )))
    }
}

/// The canvas page for the design at `path`: the design fills the window,
/// drawn by the design script.
pub fn page(path: &str) -> String {
    let path = path.trim_start_matches('/');
    let file = path.rsplit('/').next().unwrap_or(path);
    let name = file.strip_suffix(".design").unwrap_or(file);
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>{name}</title>
<meta name="viewport" content="width=device-width, initial-scale=1">
<link rel="stylesheet" href="/__devpage/style.css">
</head><body class="design-page">
<canvas class="design" src="/{path}" role="img" aria-label="{name}"></canvas>
<script type="module" src="/__devpage/design.js"></script>
</body></html>"#,
        name = crate::render::escape(name),
        path = crate::render::escape(path),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Document, Entry};
    use crate::index::Index;
    use crate::server::{Identity, router_with_index};
    use prost::Message;
    use std::time::Duration;

    const BUTTON: &str = r##"<Frame props={{ label: "Button" }} layout="row" padding={[12, 20]} fill="#2563eb">
  <Text color="#ffffff">{label}</Text>
</Frame>
"##;
    const SCREEN: &str = r##"import Button from "./button.design"

<Frame width={200} height={120} layout="column" gap={8} padding={16} fill="#f8fafc">
  <Text>Cart</Text>
  <Button label="Pay" />
</Frame>
"##;
    const OTHER: &str = r##"<Frame width={50} height={50} fill="#000000" />
"##;

    fn served(name: &str) -> PathBuf {
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
    fn save(designs: &Designs, file: &Path, text: &str) {
        std::fs::write(file, text).unwrap();
        designs.changed(file);
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

    #[tokio::test]
    async fn no_design_file_stops_the_store() {
        let dir = served("hostile");
        let designs = Designs::new(dir.clone());
        let other = dir.join("app/other.design");
        assert!(designs.png("app/other.design".into()).await.is_ok());
        // A colour with a pasted zero-width space, saved over a design held
        // in memory: what once panicked the watcher's thread.
        save(&designs, &other, &OTHER.replace("#000000", "#000\u{200b}"));
        let problems = designs.png("app/other.design".into()).await.unwrap_err();
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
                designs.png("app/other.design".into()).await.is_err(),
                "{text:.40}"
            );
        }
        // And the store still sees saves afterwards.
        save(&designs, &other, &OTHER.replace("{50}", "{20}"));
        let png = designs.png("app/other.design".into()).await.unwrap();
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

    #[test]
    fn a_design_outside_the_served_folder_is_never_read() {
        let dir = served("outside");
        std::fs::write(dir.join("../outside.design"), OTHER).unwrap();
        let designs = Designs::new(dir.join("app"));
        assert!(designs.resolve("../outside.design").is_err());
    }

    async fn start(dir: &Path, entry: &str) -> (String, Designs) {
        let document = Arc::new(Document::load(&dir.join(entry), dir).unwrap());
        let index = Arc::new(Index::build(dir));
        let designs = Designs::new(dir.to_path_buf());
        let router = router_with_index(
            Arc::new(Entry::new(document)),
            index,
            designs.clone(),
            broadcast::channel(16).0,
            Identity::Local("T".into()),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await.ok() });
        (address, designs)
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

    /// A gRPC-Web `Watch` call, read frame by frame.
    struct Stream {
        response: reqwest::Response,
        buffer: Vec<u8>,
    }

    impl Stream {
        async fn open(address: &str, paths: &[&str]) -> Stream {
            let request = WatchRequest {
                paths: paths.iter().map(|p| p.to_string()).collect(),
            }
            .encode_to_vec();
            let mut body = vec![0];
            body.extend((request.len() as u32).to_be_bytes());
            body.extend(request);
            let response = reqwest::Client::new()
                .post(format!("{address}/devpage.design.v1.Designs/Watch"))
                .header("content-type", "application/grpc-web+proto")
                .header("x-grpc-web", "1")
                .body(body)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            Stream {
                response,
                buffer: Vec::new(),
            }
        }

        /// The next frame's body: `Ok(body)`, `Err(trailers)` at the end, or
        /// `None` when nothing arrives within a second.
        async fn next(&mut self) -> Option<Result<Body, String>> {
            self.update()
                .await
                .map(|update| update.map(|update| update.body.unwrap()))
        }

        /// The next frame, whole.
        async fn update(&mut self) -> Option<Result<Update, String>> {
            loop {
                if self.buffer.len() >= 5 {
                    let length = u32::from_be_bytes(self.buffer[1..5].try_into().unwrap()) as usize;
                    if self.buffer.len() >= 5 + length {
                        let flag = self.buffer[0];
                        let payload: Vec<u8> = self.buffer.drain(..5 + length).skip(5).collect();
                        return Some(if flag & 0x80 != 0 {
                            Err(String::from_utf8_lossy(&payload).into_owned())
                        } else {
                            Ok(Update::decode(&payload[..]).unwrap())
                        });
                    }
                }
                let chunk =
                    tokio::time::timeout(Duration::from_secs(1), self.response.chunk()).await;
                match chunk {
                    Ok(Ok(Some(bytes))) => self.buffer.extend_from_slice(&bytes),
                    Ok(_) | Err(_) => return None,
                }
            }
        }
    }

    #[test]
    fn the_canvas_page_is_titled_without_the_extension() {
        let page = page("/app/checkout.design");
        assert!(page.contains("<title>checkout</title>"), "{page}");
        assert!(page.contains(r#"src="/app/checkout.design""#), "{page}");
    }

    #[tokio::test]
    async fn the_watch_stream_sends_the_scene_then_only_what_changed() {
        let dir = served("watch");
        let (address, designs) = start(&dir, "app/screen.design").await;
        let mut stream = Stream::open(&address, &["/app/screen.design"]).await;

        let Some(Ok(Body::Scene(scene))) = stream.next().await else {
            panic!("the first update is the scene")
        };
        assert_eq!(scene.children.len(), 2);

        // A save of the imported button: a patch of the button's instance only.
        save(
            &designs,
            &dir.join("app/button.design"),
            &BUTTON.replace("#2563eb", "#16a34a"),
        );
        let Some(Ok(Body::Patch(patch))) = stream.next().await else {
            panic!("a patch")
        };
        assert_eq!(
            patch
                .replace
                .iter()
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>(),
            vec!["0.1"]
        );

        // A save that changes nothing in the scene sends nothing.
        save(
            &designs,
            &dir.join("app/button.design"),
            &format!("// a comment\n{}", BUTTON.replace("#2563eb", "#16a34a")),
        );
        assert!(stream.next().await.is_none());

        // Problems, as their own update.
        save(
            &designs,
            &dir.join("app/screen.design"),
            &SCREEN.replace("<Button label", "<Button lable"),
        );
        let Some(Ok(Body::Problems(problems))) = stream.next().await else {
            panic!("problems")
        };
        assert!(problems.problems[0].contains("lable"), "{problems:?}");

        // The typo undone: the scene is the one last sent, and an empty patch
        // says the design is valid again.
        save(&designs, &dir.join("app/screen.design"), SCREEN);
        let Some(Ok(Body::Patch(patch))) = stream.next().await else {
            panic!("an empty patch after the undo")
        };
        assert!(patch.replace.is_empty(), "{patch:?}");

        // Problems again, then a fix that changes the scene: a patch against
        // the last scene sent.
        save(
            &designs,
            &dir.join("app/screen.design"),
            &SCREEN.replace("<Button label", "<Button lable"),
        );
        assert!(matches!(stream.next().await, Some(Ok(Body::Problems(_)))));
        save(
            &designs,
            &dir.join("app/screen.design"),
            &SCREEN.replace("Cart", "Basket"),
        );
        let Some(Ok(Body::Patch(patch))) = stream.next().await else {
            panic!("a patch after the fix")
        };
        assert_eq!(patch.replace[0].id, "0.0");

        // Deleted: the problem, then the end of the stream, since no design
        // is left.
        std::fs::remove_file(dir.join("app/screen.design")).unwrap();
        designs.changed(&dir.join("app/screen.design"));
        let Some(Ok(Body::Problems(problems))) = stream.next().await else {
            panic!("the deleted design's problem")
        };
        assert!(
            problems.problems[0].contains("no such file"),
            "{problems:?}"
        );
        assert!(
            matches!(stream.next().await, Some(Err(_))),
            "the trailers end it"
        );
    }

    #[tokio::test]
    async fn one_stream_watches_every_design_a_page_shows() {
        let dir = served("many");
        std::fs::write(
            dir.join("app/other.design"),
            SCREEN.replace("Cart", "Other"),
        )
        .unwrap();
        let (address, designs) = start(&dir, "app/screen.design").await;
        let mut stream = Stream::open(
            &address,
            &[
                "/app/screen.design",
                "app/other.design",
                "app/screen.design",
            ],
        )
        .await;

        // Each design's scene, named as the request named it, once.
        let mut first = Vec::new();
        for _ in 0..2 {
            let Some(Ok(update)) = stream.update().await else {
                panic!("a scene for each design")
            };
            assert!(matches!(update.body, Some(Body::Scene(_))), "{update:?}");
            first.push(update.path);
        }
        assert_eq!(first, ["/app/screen.design", "app/other.design"]);
        assert!(stream.update().await.is_none(), "no third scene");

        // An edit to one: a patch naming it.
        save(
            &designs,
            &dir.join("app/other.design"),
            &SCREEN.replace("Cart", "Elsewhere"),
        );
        let Some(Ok(update)) = stream.update().await else {
            panic!("a patch")
        };
        assert_eq!(update.path, "app/other.design");
        assert!(matches!(update.body, Some(Body::Patch(_))));

        // An edit to the button both use: a patch for each.
        save(
            &designs,
            &dir.join("app/button.design"),
            &BUTTON.replace("#2563eb", "#16a34a"),
        );
        let mut patched = Vec::new();
        for _ in 0..2 {
            let Some(Ok(update)) = stream.update().await else {
                panic!("a patch for each design")
            };
            assert!(matches!(update.body, Some(Body::Patch(_))));
            patched.push(update.path);
        }
        patched.sort();
        assert_eq!(patched, ["/app/screen.design", "app/other.design"]);

        // One deleted: its last update says so, and the stream goes on.
        std::fs::remove_file(dir.join("app/other.design")).unwrap();
        designs.changed(&dir.join("app/other.design"));
        let Some(Ok(update)) = stream.update().await else {
            panic!("the deleted design's problem")
        };
        assert_eq!(update.path, "app/other.design");
        assert!(update.gone);
        let Some(Body::Problems(problems)) = update.body else {
            panic!("its problem")
        };
        assert!(
            problems.problems[0].contains("no such file"),
            "{problems:?}"
        );
        save(
            &designs,
            &dir.join("app/screen.design"),
            &SCREEN.replace("Cart", "Basket"),
        );
        let Some(Ok(update)) = stream.update().await else {
            panic!("the stream goes on for the design left")
        };
        assert_eq!(update.path, "/app/screen.design");
        assert!(!update.gone);
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
        let problems = crate::check::document(&document, false).await;
        assert_eq!(problems.len(), 1, "{problems:?}");
        let shown = problems[0].to_string();
        assert!(
            shown.starts_with("page.md:3: broken design app/screen.design — app/screen.design:5:"),
            "{shown}"
        );
        assert!(shown.contains("lable"), "{shown}");

        let design = Document::load(&dir.join("app/screen.design"), &dir).unwrap();
        let problems = crate::check::document(&design, false).await;
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].to_string().starts_with("app/screen.design:5:"),
            "{problems:?}"
        );
    }
}

#[cfg(test)]
mod canvas_tests;
