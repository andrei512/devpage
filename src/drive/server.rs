//! The HTTP server: pages rendered on request, the page assets, and a stream
//! that tells open pages a watched file changed.

#[cfg(test)]
pub(crate) mod design_tests;
mod watch_stream;

use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Json;
use axum::Router;
use axum::extract::Query;
use axum::extract::{Path, RawQuery, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use serde::Deserialize;

use crate::answer::{Answer, Answers};
use crate::changes::Changes;
use crate::check::Preview;
use crate::comments::{self, Anchor, Comments, Identity};
use crate::designs::Designs;
use crate::document::Document;
use crate::folder::Folder;
use crate::search::Search;
use crate::slide_png::Shots;

#[derive(Clone)]
struct AppState {
    folder: Arc<Folder>,
    modules: crate::widgets::Modules,
    search: Option<Arc<Search>>,
    changes: Changes,
    /// `None` when comments are off: the pages show none and every comment
    /// route answers 404.
    comments: Option<Arc<Commenting>>,
    designs: Designs,
    /// The design store's changes, as the watch streams hear them.
    design_updates: tokio::sync::broadcast::Sender<String>,
    /// Slides as images: the Chrome that draws them and what it drew.
    shots: Shots,
    /// The problems each page lists above itself.
    preview: Arc<Preview>,
}

impl AppState {
    /// The comments, or — when they are off — the 404 every comment route
    /// answers, as if it weren't there.
    #[allow(clippy::result_large_err)]
    fn comments(&self) -> Result<&Commenting, Response> {
        self.comments
            .as_deref()
            .ok_or_else(|| (StatusCode::NOT_FOUND, "comments are off").into_response())
    }

    fn comments_on(&self) -> bool {
        self.comments.is_some()
    }
}

/// The author of a write, as `identity` names it from the request's
/// `headers`: refused when it needs a header the request lacks or leaves
/// blank.
pub fn author(identity: &Identity, headers: &HeaderMap) -> Result<String, comments::Refused> {
    identity.author(|name| headers.get(name).and_then(|value| value.to_str().ok()))
}

/// A refused comment request as the page's routes answer it: `401` with no
/// author, `422` for a page that isn't a file in the served folder, `400`
/// otherwise.
fn refused(refused: comments::Refused) -> Response {
    let status = match refused {
        comments::Refused::Unidentified(_) => StatusCode::UNAUTHORIZED,
        comments::Refused::NotAPage(_) => StatusCode::UNPROCESSABLE_ENTITY,
        comments::Refused::NoThread(_) | comments::Refused::Failed(_) => StatusCode::BAD_REQUEST,
    };
    (status, refused.to_string()).into_response()
}

/// Comments on: the threads, announced as they are written, and who writes
/// them.
pub struct Commenting {
    threads: Comments,
    identity: Identity,
}

impl Commenting {
    pub fn new(threads: Comments, identity: Identity) -> Self {
        Self { threads, identity }
    }
}

/// Everything one server is made of: one document under its served folder,
/// served in one of four ways, each a constructor that takes exactly what
/// varies — [`App::local`] for `devpage`, [`App::printing`] for what Chrome
/// prints and records, [`App::published`] for one version in server mode,
/// and [`App::testing`] for the tests.
pub struct App {
    /// The served folder, opened on the entry document, which it follows
    /// as its file changes.
    folder: Arc<Folder>,
    /// What was read and compiled of the served folder's widgets.
    modules: crate::widgets::Modules,
    /// The search index of what the entry document publishes, or `None`:
    /// every search then finds nothing.
    search: Option<Arc<Search>>,
    designs: Designs,
    /// What tells open pages there is something new to show.
    changes: Changes,
    /// `None` when comments are off.
    comments: Option<Arc<Commenting>>,
    /// The Chrome that draws slides as PNGs, and what it drew.
    shots: Shots,
    /// The problems each page lists.
    preview: Arc<Preview>,
}

impl App {
    /// `devpage`: `document` with the search index of what it publishes, its
    /// own design store, announcing on `changes`, its problems found by
    /// `preview`, and `comments` — or none, `--no-comments`.
    pub fn local(
        document: Arc<Document>,
        changes: Changes,
        preview: Arc<Preview>,
        comments: Option<Commenting>,
    ) -> Self {
        let designs = Designs::new(&document.served);
        Self::of(document, designs, changes, preview, comments).searched()
    }

    /// What `devpage pdf`, `devpage video` and a version's download serve
    /// only Chrome: no search index, a bus of its own, problems checked
    /// against the network and announced to no one, and the served folder's
    /// own comments, shown as they are.
    pub fn printing(document: Arc<Document>) -> Self {
        let changes = Changes::default();
        let comments = Commenting::new(
            Comments::new(
                Arc::new(comments::yaml::Yaml::new(&document.served)),
                Arc::new(changes.clone()),
                document.served.clone(),
            ),
            Identity::Local(String::new()),
        );
        let preview = Arc::new(Preview::new(
            crate::http::external(None),
            Arc::new(crate::terminal::Terminal),
        ));
        let designs = Designs::new(&document.served);
        Self::of(document, designs, changes, preview, Some(comments))
    }

    /// One published version, `document` in its unpacked folder: the search
    /// index of what it publishes, its `designs` from the store every
    /// version shares, announcing on `changes`, asking no external address —
    /// they are the CLI's to ask, before it uploads — and `comments`, or none
    /// for a version that never changes.
    pub fn published(
        document: Arc<Document>,
        designs: Designs,
        changes: Changes,
        comments: Option<Commenting>,
    ) -> Self {
        let preview = Arc::new(Preview::new(
            crate::http::offline(),
            Arc::new(crate::terminal::Terminal),
        ));
        Self::of(document, designs, changes, preview, comments).searched()
    }

    /// `document` for a test: no search index, a design store of its own,
    /// announcing on `changes`, its problems found without the network and
    /// reported to no one, and comments off until [`App::with_comments`].
    #[cfg(test)]
    pub fn testing(document: Arc<Document>, changes: Changes) -> Self {
        let designs = Designs::new(&document.served);
        let preview = crate::check::testing::preview();
        Self::of(document, designs, changes, preview, None)
    }

    /// Comments on, written by `identity`, its threads kept in memory.
    #[cfg(test)]
    pub fn with_comments(self, identity: Identity) -> Self {
        let store = Arc::new(comments::memory::Memory::default());
        self.commenting(store, identity)
    }

    /// Comments on, written by `identity`, its threads in the served
    /// folder's `.devpage/` as `devpage` keeps them: for a test whose threads
    /// are files, written beforehand or heard by a watcher.
    #[cfg(test)]
    pub fn with_comments_on_disk(self, identity: Identity) -> Self {
        let store = Arc::new(comments::yaml::Yaml::new(self.folder.served()));
        self.commenting(store, identity)
    }

    #[cfg(test)]
    fn commenting(self, store: Arc<dyn comments::Store>, identity: Identity) -> Self {
        let threads = Comments::new(
            store,
            Arc::new(self.changes.clone()),
            self.folder.served().clone(),
        );
        Self {
            comments: Some(Arc::new(Commenting::new(threads, identity))),
            ..self
        }
    }

    /// The design store `designs` in place of its own, for a test that tells
    /// it of a saved file as the watcher would.
    #[cfg(test)]
    pub fn with_designs(self, designs: Designs) -> Self {
        Self { designs, ..self }
    }

    fn of(
        document: Arc<Document>,
        designs: Designs,
        changes: Changes,
        preview: Arc<Preview>,
        comments: Option<Commenting>,
    ) -> Self {
        Self {
            folder: Arc::new(Folder::new(document)),
            modules: crate::widgets::Modules::default(),
            search: None,
            designs,
            shots: Shots::new(&changes),
            comments: comments.map(Arc::new),
            changes,
            preview,
        }
    }

    /// With the search index of what the entry document publishes.
    fn searched(self) -> Self {
        let search = Search::build(&self.folder, self.folder.entry_file());
        Self {
            search: Some(Arc::new(search)),
            ..self
        }
    }

    /// Starts the watcher this server pairs with: its listeners hold the
    /// same served folder, design store, widget modules, change bus and
    /// search index, and the same comments, so a thread the server wrote is
    /// announced once, by the server.
    pub fn watch(&self) -> anyhow::Result<crate::watch::Watch> {
        crate::watch::start(devpage_core::watch::Listeners::new(
            self.folder.clone(),
            self.designs.clone(),
            self.modules.clone(),
            self.comments
                .as_ref()
                .map(|comments| comments.threads.clone()),
            Arc::new(self.changes.clone()),
            self.search.clone(),
        ))
    }

    /// The Chrome that draws this server's slides as PNGs, for its owner to
    /// close when the server stops.
    pub fn shots(&self) -> Shots {
        self.shots.clone()
    }
}

/// The server `app` describes, logging nothing.
pub fn router(app: App) -> Router {
    let App {
        folder,
        modules,
        search,
        designs,
        changes,
        comments,
        shots,
        preview,
    } = app;
    routes(AppState {
        comments,
        design_updates: watch_stream::hear(&designs),
        designs,
        folder,
        modules,
        search,
        changes,
        shots,
        preview,
    })
}

/// `router`, logging each request, as `devpage` does.
pub fn logged(router: Router) -> Router {
    router.layer(middleware::from_fn(log_request))
}

/// Every route, on `state`, logging nothing.
fn routes(state: AppState) -> Router {
    let designs = state.designs.clone();
    Router::new()
        .route("/", get(home))
        .route(
            "/__devpage/comments",
            get(list_comments).post(create_comment),
        )
        .route("/__devpage/comments/reply", post(reply_comment))
        .route("/__devpage/comments/resolve", post(resolve_comment))
        .route("/__devpage/search", get(search))
        .route("/__devpage/system-widgets/{name}", get(system_widget))
        .route("/__devpage/widgets/{*path}", get(project_widget))
        .route("/__devpage/{asset}", get(asset))
        .route("/{*path}", get(file))
        // Every route so far answers with a validator; the streams below
        // never end, so the middleware, which reads a whole body, skips them.
        .route_layer(middleware::from_fn(crate::validators::validate))
        .route("/__devpage/events", get(events))
        .route_service(
            "/devpage.design.v1.Designs/Watch",
            tower::ServiceBuilder::new()
                .layer(tonic_web::GrpcWebLayer::new())
                .service(devpage_design::proto::designs_server::DesignsServer::new(
                    watch_stream::Watch::new(designs.clone(), state.design_updates.clone()),
                )),
        )
        .with_state(state)
}

/// Logs each request with its status and how long the response took to start.
async fn log_request(request: Request, next: Next) -> Response {
    let (method, path) = (request.method().clone(), request.uri().path().to_string());
    let started = Instant::now();
    let response = next.run(request).await;
    eprintln!(
        "{method} {path} → {} in {:.1} ms",
        response.status().as_u16(),
        started.elapsed().as_secs_f64() * 1000.0
    );
    response
}

/// What decides a request's answer: the server's documents, under their
/// served folder's mount.
fn answers(state: &AppState) -> Answers<'_> {
    Answers::new(
        &state.folder,
        &state.modules,
        &state.preview,
        state.comments_on(),
    )
}

/// `/`: whatever the entry document leads to now.
async fn home(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let answer = answers(&state).home();
    respond(&state, &headers, answer).await
}

/// Any other path in the served folder.
async fn file(
    State(state): State<AppState>,
    Path(path): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let answer = answers(&state).path(&path, query.as_deref());
    respond(&state, &headers, answer).await
}

/// A system widget's module.
async fn system_widget(
    State(state): State<AppState>,
    Path(name): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let answer = answers(&state).system_widget(&name, query.as_deref());
    respond(&state, &headers, answer).await
}

/// A project widget's module, or a helper's it imports.
async fn project_widget(
    State(state): State<AppState>,
    Path(path): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let answer = answers(&state).widget(&path, query.as_deref());
    respond(&state, &headers, answer).await
}

/// An answer as a response: its headers, the file's bytes read, the PNG
/// drawn.
async fn respond(state: &AppState, headers: &HeaderMap, answer: Answer) -> Response {
    match answer {
        Answer::Html(html) => Html(html).into_response(),
        Answer::Module { js, immutable } => javascript(js, immutable),
        // A module that throws is never kept: the next request may compile.
        Answer::ModuleError(js) => javascript(js, false),
        Answer::Redirect(address) => Redirect::temporary(&address).into_response(),
        Answer::NotFound(why) => (StatusCode::NOT_FOUND, why).into_response(),
        Answer::BadRequest(why) => (StatusCode::BAD_REQUEST, why).into_response(),
        Answer::Failed(why) => (StatusCode::INTERNAL_SERVER_ERROR, why).into_response(),
        Answer::File(file) => match tokio::fs::read(&file).await {
            Ok(bytes) => {
                let mime = mime_guess::from_path(&file).first_or_octet_stream();
                ([(header::CONTENT_TYPE, mime.to_string())], bytes).into_response()
            }
            Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
        },
        Answer::DesignPng { design, theme } => design_png(state, headers, design, theme).await,
        Answer::SlidePng { page, n } => slide_png(state, headers, &page, n).await,
    }
}

/// A module, `immutable` when what answers its address never changes, else
/// `no-cache`.
fn javascript(js: String, immutable: bool) -> Response {
    let cache = if immutable { IMMUTABLE } else { "no-cache" };
    (
        [
            (header::CONTENT_TYPE, "text/javascript"),
            (header::CACHE_CONTROL, cache),
        ],
        js,
    )
        .into_response()
}

/// The design at `design` as a PNG in `theme`. The ETag comes from the
/// scene, so a browser that has this PNG already is answered without
/// painting it again.
async fn design_png(
    state: &AppState,
    headers: &HeaderMap,
    design: String,
    theme: devpage_design::Theme,
) -> Response {
    // Resolving and painting block, so they run off the async threads.
    let designs = state.designs.clone();
    let path = design.clone();
    let tag = tokio::task::spawn_blocking(move || designs.etag(&path, theme))
        .await
        .ok()
        .flatten()
        .map(|hash| crate::validators::strong(&hash));
    if let Some(tag) = &tag
        && crate::validators::already_has(headers, tag)
    {
        let mut known = HeaderMap::new();
        if let Ok(value) = tag.parse() {
            known.insert(header::ETAG, value);
        }
        known.insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-cache"),
        );
        return crate::validators::not_modified(known);
    }
    // The PNG streams band by band as it is painted; the problems, if any,
    // are known before the first byte.
    let designs = state.designs.clone();
    let (first, started) = tokio::sync::oneshot::channel::<Result<(), String>>();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<axum::body::Bytes, std::io::Error>>(4);
    tokio::task::spawn_blocking(move || {
        struct Sink {
            tx: tokio::sync::mpsc::Sender<Result<axum::body::Bytes, std::io::Error>>,
            first: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
        }
        impl std::io::Write for Sink {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                if let Some(first) = self.first.take() {
                    let _ = first.send(Ok(()));
                }
                self.tx
                    .blocking_send(Ok(axum::body::Bytes::copy_from_slice(buf)))
                    .map_err(|_| std::io::Error::other("the client went away"))?;
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut sink = Sink {
            tx,
            first: Some(first),
        };
        let result = designs.png_to(&design, theme, &mut sink);
        if let Some(first) = sink.first.take() {
            let _ = first.send(result);
        }
    });
    match started
        .await
        .unwrap_or_else(|_| Err("painting stopped".into()))
    {
        Ok(()) => {
            let body =
                axum::body::Body::from_stream(tokio_stream::wrappers::ReceiverStream::new(rx));
            let mut response = ([(header::CONTENT_TYPE, "image/png")], body).into_response();
            if let Some(tag) = tag.and_then(|tag| tag.parse().ok()) {
                response.headers_mut().insert(header::ETAG, tag);
            }
            response
        }
        Err(problems) => (StatusCode::UNPROCESSABLE_ENTITY, problems).into_response(),
    }
}

/// Slide `n` of the deck whose page is at `page`, drawn by Chrome from these
/// routes with comments off, so a slide is shot without them. A slide drawn
/// since the last file change comes from the cache, and is a 304 for a
/// request that already holds it. Asked for by a page devpage's own Chrome
/// is shooting, it is never drawn: the cache's copy, or 508.
async fn slide_png(state: &AppState, headers: &HeaderMap, page: &str, n: usize) -> Response {
    let shot = if headers.contains_key(crate::chrome::SHOT_HEADER) {
        match state.shots.cached(page, n) {
            Some(shot) => shot,
            None => {
                return (
                    StatusCode::LOOP_DETECTED,
                    format!("{}/{n}.png isn't drawn inside another shot", &page[1..]),
                )
                    .into_response();
            }
        }
    } else {
        let shooting = AppState {
            comments: None,
            ..state.clone()
        };
        match state.shots.png(page, n, move || routes(shooting)).await {
            Ok(shot) => shot,
            Err(error) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}")).into_response();
            }
        }
    };
    let held = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|tags| tags.split(',').any(|tag| tag.trim() == shot.etag));
    let mut response = if held {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        ([(header::CONTENT_TYPE, "image/png")], shot.png).into_response()
    };
    let answer = response.headers_mut();
    if let Ok(etag) = shot.etag.parse() {
        answer.insert(header::ETAG, etag);
    }
    // Drawn as it was after 20 s, not ready: the log says what it waited for.
    if !shot.ready {
        answer.insert(
            "x-devpage-ready",
            header::HeaderValue::from_static("timeout"),
        );
    }
    response
}

/// Server-sent events: one message per change published on the server's
/// change bus.
async fn events(
    State(state): State<AppState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let heard = state.changes.subscribe();
    let stream = futures::stream::unfold(heard, |mut heard| async move {
        let change = heard.next().await?;
        Some((Ok(Event::default().data(change.message())), heard))
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

/// `Cache-Control` for what answers a current content address: it never
/// changes.
const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// A built-in asset, from the manifest `build.rs` made: `immutable` at its
/// content address; at its bare name or a stale hash, the current bytes with
/// `no-cache`, so a page from an older build still loads. Sent brotli- or
/// gzip-compressed when the browser takes it.
pub async fn asset(Path(name): Path<String>, headers: HeaderMap) -> Response {
    let Some((asset, current)) = crate::addresses::asset(&name) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let encoding =
        crate::validators::encoding(&headers, asset.brotli.is_some(), asset.gzip.is_some());
    let (body, tag) = match encoding {
        Some("br") => (asset.brotli, format!("{}-br", asset.hash)),
        Some(_) => (asset.gzip, format!("{}-gz", asset.hash)),
        None => (Some(asset.identity), asset.hash.to_string()),
    };
    let mut response = (
        [
            (header::CONTENT_TYPE, asset.mime.to_string()),
            (
                header::CACHE_CONTROL,
                if current { IMMUTABLE } else { "no-cache" }.to_string(),
            ),
            (header::VARY, "Accept-Encoding".to_string()),
            (header::ETAG, crate::validators::strong(&tag)),
        ],
        body.unwrap_or(asset.identity),
    )
        .into_response();
    if let Some(encoding) = encoding {
        response.headers_mut().insert(
            header::CONTENT_ENCODING,
            header::HeaderValue::from_static(encoding),
        );
    }
    response
}

#[derive(Deserialize)]
struct PageQuery {
    page: String,
}

#[derive(Deserialize)]
struct NewThread {
    page: String,
    anchor: Anchor,
    text: String,
}

/// A reply to a thread, or — with no text — resolving it: the thread's id
/// is all that finds it.
#[derive(Deserialize)]
struct Reply {
    id: String,
    #[serde(default)]
    text: String,
}

/// `?q=` of a search; none is an empty query.
#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
}

/// The pages a search finds, as JSON, best first: `{"pages": [...]}`.
/// Comments on or off; a server without a search index — printing's — finds
/// nothing.
async fn search(State(state): State<AppState>, Query(query): Query<SearchQuery>) -> Response {
    let answer = state
        .search
        .as_ref()
        .map(|search| search.search(&query.q))
        .unwrap_or_default();
    Json(answer).into_response()
}

async fn list_comments(State(state): State<AppState>, Query(query): Query<PageQuery>) -> Response {
    let comments = match state.comments() {
        Ok(comments) => comments,
        Err(off) => return off,
    };
    match comments.threads.list(&query.page) {
        Ok(threads) => Json(threads).into_response(),
        Err(error) => refused(error),
    }
}

async fn create_comment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(new): Json<NewThread>,
) -> Response {
    let comments = match state.comments() {
        Ok(comments) => comments,
        Err(off) => return off,
    };
    let created = author(&comments.identity, &headers).and_then(|author| {
        comments
            .threads
            .create(&new.page, new.anchor, &author, &new.text)
    });
    match created {
        Ok(id) => Json(serde_json::json!({ "id": id })).into_response(),
        Err(error) => refused(error),
    }
}

async fn reply_comment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(reply): Json<Reply>,
) -> Response {
    let comments = match state.comments() {
        Ok(comments) => comments,
        Err(off) => return off,
    };
    let replied = author(&comments.identity, &headers)
        .and_then(|author| comments.threads.reply(&reply.id, &author, &reply.text));
    match replied {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => refused(error),
    }
}

async fn resolve_comment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(reply): Json<Reply>,
) -> Response {
    let comments = match state.comments() {
        Ok(comments) => comments,
        Err(off) => return off,
    };
    let resolved =
        author(&comments.identity, &headers).and_then(|_| comments.threads.resolve(&reply.id));
    match resolved {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => refused(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folder::ServedFolder;
    use serde_json::{Value, json};

    /// A served folder holding one page, served by the real router on a free
    /// port with its threads in memory and no watcher, with the given comment
    /// author — or `None`, comments off. Also the bus the server announces on.
    async fn serve_comments(
        name: &str,
        comments: Option<Identity>,
    ) -> (String, std::path::PathBuf) {
        let (address, root, _) = serve_comments_on(name, comments).await;
        (address, root)
    }

    async fn serve_comments_on(
        name: &str,
        comments: Option<Identity>,
    ) -> (String, std::path::PathBuf, crate::changes::Changes) {
        let root =
            std::env::temp_dir().join(format!("devpage-routes-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("a.md"),
            "# A\n\nThe client retries every write.\n",
        )
        .unwrap();
        let document = Arc::new(
            Document::load(&root.join("a.md"), &ServedFolder::new(&root).unwrap()).unwrap(),
        );
        let changes = crate::changes::Changes::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let app = App::testing(document, changes.clone());
        let router = router(match comments {
            Some(identity) => app.with_comments(identity),
            None => app,
        });
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        (address, root, changes)
    }

    fn new_thread() -> Value {
        json!({
            "page": "/a.md",
            "anchor": { "quote": "retries", "prefix": "The client ", "suffix": " every write." },
            "text": "Every write?",
        })
    }

    #[tokio::test]
    async fn a_thread_is_created_replied_to_and_resolved_over_http() {
        let (address, root) = serve_comments("life", Some(Identity::Local("Ana".into()))).await;
        let client = reqwest::Client::new();
        let created: Value = client
            .post(format!("{address}/__devpage/comments"))
            .json(&new_thread())
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let id = created["id"].as_str().unwrap().to_string();
        let reply = client
            .post(format!("{address}/__devpage/comments/reply"))
            .json(&json!({ "id": id, "text": "Only the outbox." }))
            .send()
            .await
            .unwrap();
        assert_eq!(reply.status(), 204);
        let listed: Value = client
            .get(format!("{address}/__devpage/comments?page=/a.md"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(listed[0]["id"], id.as_str());
        assert_eq!(listed[0]["anchor"]["quote"], "retries");
        assert_eq!(listed[0]["thread"][0]["author"], "Ana");
        assert_eq!(listed[0]["thread"][1]["text"], "Only the outbox.");
        assert!(!root.join(".devpage").exists(), "kept in memory");

        let resolved = client
            .post(format!("{address}/__devpage/comments/resolve"))
            .json(&json!({ "id": id }))
            .send()
            .await
            .unwrap();
        assert_eq!(resolved.status(), 204);
        let listed: Value = client
            .get(format!("{address}/__devpage/comments?page=/a.md"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(listed, json!([]));
    }

    #[tokio::test]
    async fn a_server_and_the_watcher_its_app_starts_announce_a_write_once() {
        use crate::changes::Change;
        let dir = std::env::temp_dir().join(format!("devpage-paired-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        std::fs::write(dir.join("a.md"), "# A\n\nThe client retries every write.\n").unwrap();
        let document =
            Arc::new(Document::load(&dir.join("a.md"), &ServedFolder::new(&dir).unwrap()).unwrap());
        let changes = crate::changes::Changes::default();
        let app = App::testing(document, changes.clone())
            .with_comments_on_disk(Identity::Local(String::new()));
        let _watcher = app.watch().unwrap();
        let router = router(app);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await.ok() });
        // Let the watcher settle on the new `.devpage/` before listening.
        tokio::time::sleep(Duration::from_millis(300)).await;

        let mut heard = changes.subscribe();
        let created = reqwest::Client::new()
            .post(format!("{address}/__devpage/comments"))
            .json(&new_thread())
            .send()
            .await
            .unwrap();
        assert_eq!(created.status(), 200);
        // Long past the watcher's burst: its echo of the write would be here.
        tokio::time::sleep(Duration::from_millis(800)).await;
        let page = Change::Comments("/a.md".into());
        let announced = heard.drain().into_iter().filter(|c| *c == page).count();
        assert_eq!(announced, 1, "the server's announcement, and no echo");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn each_write_reaches_open_pages_without_a_watcher() {
        use crate::changes::Change;
        let (address, _, changes) =
            serve_comments_on("announce", Some(Identity::Local("Ana".into()))).await;
        let mut heard = changes.subscribe();
        let client = reqwest::Client::new();
        let created: Value = client
            .post(format!("{address}/__devpage/comments"))
            .json(&new_thread())
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let id = created["id"].as_str().unwrap();
        for action in ["reply", "resolve"] {
            let done = client
                .post(format!("{address}/__devpage/comments/{action}"))
                .json(&json!({ "id": id, "text": "x" }))
                .send()
                .await
                .unwrap();
            assert_eq!(done.status(), 204, "{action}");
        }
        // A refused write and a read announce nothing.
        let refused = client
            .post(format!("{address}/__devpage/comments/reply"))
            .json(&json!({ "id": id, "text": "gone" }))
            .send()
            .await
            .unwrap();
        assert_eq!(refused.status(), 400);
        client
            .get(format!("{address}/__devpage/comments?page=/a.md"))
            .send()
            .await
            .unwrap();
        let page = Change::Comments("/a.md".into());
        assert_eq!(heard.drain(), [page.clone(), page.clone(), page]);
    }

    #[tokio::test]
    async fn deployed_writes_need_the_header_and_are_stamped_with_it() {
        let identity = Identity::Header("X-Forwarded-Email".into());
        let (address, _) = serve_comments("deployed", Some(identity)).await;
        let client = reqwest::Client::new();
        let refused = client
            .post(format!("{address}/__devpage/comments"))
            .json(&new_thread())
            .send()
            .await
            .unwrap();
        assert_eq!(refused.status(), 401);
        let created: Value = client
            .post(format!("{address}/__devpage/comments"))
            .header("X-Forwarded-Email", "ana@example.com")
            .json(&new_thread())
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let id = created["id"].as_str().unwrap();
        for action in ["reply", "resolve"] {
            let refused = client
                .post(format!("{address}/__devpage/comments/{action}"))
                .json(&json!({ "id": id, "text": "x" }))
                .send()
                .await
                .unwrap();
            assert_eq!(refused.status(), 401, "{action} without the header");
        }
        // Reading needs no header: the proxy decides who may read.
        let listed: Value = client
            .get(format!("{address}/__devpage/comments?page=/a.md"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(listed[0]["thread"][0]["author"], "ana@example.com");
    }

    #[tokio::test]
    async fn a_thread_on_a_page_that_isnt_a_file_in_the_served_folder_is_refused() {
        let (address, _) = serve_comments("outside", Some(Identity::Local("Ana".into()))).await;
        let mut thread = new_thread();
        for page in ["/../escape.md", "/missing.md", "/"] {
            thread["page"] = json!(page);
            let refused = reqwest::Client::new()
                .post(format!("{address}/__devpage/comments"))
                .json(&thread)
                .send()
                .await
                .unwrap();
            assert_eq!(refused.status(), 422, "{page}");
        }
    }

    #[tokio::test]
    async fn with_comments_off_every_comment_route_is_404_and_nothing_is_written() {
        let (address, root) = serve_comments("off", None).await;
        let client = reqwest::Client::new();
        let listed = client
            .get(format!("{address}/__devpage/comments?page=/a.md"))
            .send()
            .await
            .unwrap();
        assert_eq!(listed.status(), 404);
        let created = client
            .post(format!("{address}/__devpage/comments"))
            .json(&new_thread())
            .send()
            .await
            .unwrap();
        assert_eq!(created.status(), 404);
        for action in ["reply", "resolve"] {
            let refused = client
                .post(format!("{address}/__devpage/comments/{action}"))
                .json(&json!({ "id": "x", "text": "hi" }))
                .send()
                .await
                .unwrap();
            assert_eq!(refused.status(), 404, "{action}");
        }
        assert!(!root.join(".devpage").exists());
        // The page itself is served, and carries the setting.
        let page = client
            .get(format!("{address}/a.md"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(page.contains("data-no-comments"), "{page}");
        assert!(!page.contains("comment-mode"), "{page}");
    }

    #[test]
    fn deployed_authors_come_from_the_proxy_header_trimmed_or_are_refused() {
        let deployed = Identity::Header("X-Forwarded-Email".into());
        let mut headers = HeaderMap::new();
        for blank in [None, Some(""), Some("   ")] {
            if let Some(blank) = blank {
                headers.insert("x-forwarded-email", blank.parse().unwrap());
            }
            let refused = author(&deployed, &headers).unwrap_err();
            assert!(
                matches!(refused, comments::Refused::Unidentified(_)),
                "{blank:?}"
            );
        }
        headers.insert("x-forwarded-email", " ana@example.com ".parse().unwrap());
        assert_eq!(author(&deployed, &headers).unwrap(), "ana@example.com");
        let local = Identity::Local("Andrei".into());
        assert_eq!(author(&local, &headers).unwrap(), "Andrei");
    }

    /// Serves `dir` as the served folder, with `entry` as the entry document,
    /// and returns the server's address.
    async fn serve(dir: &std::path::Path, entry: &str) -> String {
        let document =
            Arc::new(Document::load(&dir.join(entry), &ServedFolder::new(dir).unwrap()).unwrap());
        let changes = crate::changes::Changes::default();
        let router = router(
            App::testing(document, changes).with_comments_on_disk(Identity::Local("T".into())),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await.ok() });
        address
    }

    #[tokio::test]
    async fn a_search_answers_pages_as_json_whether_comments_are_on_or_off() {
        let dir = std::env::temp_dir().join(format!("devpage-search-route-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        std::fs::write(
            dir.join("devpage.yml"),
            "kind: site\ntitle: T\nroot: a.md\n",
        )
        .unwrap();
        std::fs::write(dir.join("a.md"), "# A\n\n## Topics\n\n- [Retries](b.md)\n").unwrap();
        std::fs::write(
            dir.join("b.md"),
            "# Retries\n\n## The outbox\n\nThe client retries every write & read from its outbox.\n",
        )
        .unwrap();
        let document = Arc::new(Document::load(&dir, &ServedFolder::new(&dir).unwrap()).unwrap());
        let changes = crate::changes::Changes::default();
        let router = router(App::local(
            document,
            changes,
            crate::check::testing::preview(),
            None,
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await.ok() });

        // `q` as it goes in the address, already escaped.
        let search = |q: &str| {
            let url = format!("{address}/__devpage/search?q={q}");
            async move {
                let response = reqwest::get(url).await.unwrap();
                assert_eq!(response.status(), 200);
                response.json::<Value>().await.unwrap()
            }
        };
        assert_eq!(
            search("outbx").await,
            json!({ "pages": [{
                "title": "Retries",
                "url": "/b.md",
                "trail": ["Retries"],
                "document": { "title": "T", "path": "/devpage.yml" },
                "kind": "page",
                "title_match": false,
                "lede": "The client retries every write & read from its outbox.",
                "sections": [{
                    "heading": "The outbox",
                    "path": ["Retries", "The outbox"],
                    "url": "/b.md#the-outbox",
                    "snippet": "The client retries every write &amp; read from its <mark>outbox</mark>.",
                    "words": ["outbox"],
                }],
                "more": 0,
            }] })
        );
        assert_eq!(search("%20%20").await, json!({ "pages": [] }));
        assert_eq!(search("").await, json!({ "pages": [] }));
        let (status, _) = get(&format!("{address}/__devpage/search")).await;
        assert_eq!(status, 200, "no q is an empty query");
    }

    async fn get(url: &str) -> (u16, String) {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let response = client.get(url).send().await.unwrap();
        let status = response.status().as_u16();
        let location = response
            .headers()
            .get("location")
            .map(|value| value.to_str().unwrap().to_string());
        let body = response.text().await.unwrap();
        (status, location.unwrap_or(body))
    }

    fn served(name: &str) -> std::path::PathBuf {
        let top =
            std::env::temp_dir().join(format!("devpage-server-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&top);
        let dir = top.join("served");
        std::fs::create_dir_all(dir.join("docs/guide")).unwrap();
        std::fs::create_dir_all(dir.join("notes")).unwrap();
        std::fs::write(top.join("secret.md"), "# Secret\n").unwrap();
        std::fs::write(dir.join("docs/README.md"), "# Home\n").unwrap();
        std::fs::write(dir.join("docs/guide/step.md"), "# Step\n").unwrap();
        std::fs::write(
            dir.join("docs/devpage.yml"),
            "kind: site\ntitle: Docs\nroot: README.md\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("notes/talk.yml"),
            "kind: slides\ntitle: Talk\nslides: [one.md]\n",
        )
        .unwrap();
        std::fs::write(dir.join("notes/one.md"), "# One\n").unwrap();
        std::fs::write(dir.join("notes/loose.md"), "# Loose\n").unwrap();
        std::fs::write(
            dir.join("notes/broken.yml"),
            "kind: doc\ntitle: B\nchapters: [gone.md]\n",
        )
        .unwrap();
        std::fs::write(dir.join("notes/ci.yml"), "on: push\n").unwrap();
        dir.canonicalize().unwrap()
    }

    #[tokio::test]
    async fn nothing_outside_the_served_folder_is_served() {
        let dir = served("outside");
        std::os::unix::fs::symlink(dir.join("../secret.md"), dir.join("docs/escape.md")).unwrap();
        let base = serve(&dir, "docs").await;
        for path in [
            "/../secret.md",
            "/%2E%2E/secret.md",
            "/docs/escape.md",
            "/docs/missing.md",
        ] {
            let (status, body) = get(&format!("{base}{path}")).await;
            assert_eq!(status, 404, "{path}: {body}");
        }
    }

    /// What `/` answers once a watched entry file settles: polls until
    /// `expected` shows up in the answer, or gives up after two seconds.
    async fn home_becomes(address: &str, expected: &str) -> (u16, String) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let answer = get(&format!("{address}/")).await;
            if answer.1.contains(expected) || Instant::now() > deadline {
                return answer;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    #[tokio::test]
    async fn the_entry_document_follows_its_file_without_a_restart() {
        let dir = std::env::temp_dir().join(format!("devpage-entry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        std::fs::write(dir.join("a.md"), "# A").unwrap();
        std::fs::write(dir.join("b.md"), "# B").unwrap();
        let site = dir.join("devpage.yml");
        std::fs::write(&site, "kind: site\ntitle: T\nroot: a.md\n").unwrap();

        let document = Arc::new(Document::load(&site, &ServedFolder::new(&dir).unwrap()).unwrap());
        let changes = crate::changes::Changes::default();
        let app = App::testing(document, changes.clone())
            .with_comments_on_disk(Identity::Local("T".into()));
        let _watcher = app.watch().unwrap();
        let router = router(app);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await.ok() });
        assert_eq!(get(&format!("{address}/")).await, (307, "/a.md".into()));
        tokio::time::sleep(Duration::from_millis(300)).await;

        // A new root: `/` follows it.
        std::fs::write(&site, "kind: site\ntitle: T\nroot: b.md\n").unwrap();
        assert_eq!(home_becomes(&address, "/b.md").await, (307, "/b.md".into()));

        // An invalid file doesn't stop devpage: `/` and the site's pages say why.
        std::fs::write(&site, "kind: site\ntitle: T\n").unwrap();
        let (status, body) = home_becomes(&address, "not a valid document").await;
        assert_eq!(status, 200);
        assert!(
            body.contains("/devpage.yml is not a valid document"),
            "{body}"
        );
        assert!(body.contains("missing field"), "{body}");
        let (_, page) = get(&format!("{address}/a.md")).await;
        assert!(page.contains("not a valid document"), "{page}");

        // Fixing it recovers.
        std::fs::write(&site, "kind: site\ntitle: T\nroot: a.md\n").unwrap();
        assert_eq!(home_becomes(&address, "/a.md").await, (307, "/a.md".into()));
        let (_, page) = get(&format!("{address}/a.md")).await;
        assert!(!page.contains("not a valid document"), "{page}");
    }

    #[tokio::test]
    async fn a_widgets_module_is_javascript_immutable_only_at_its_current_address() {
        let dir = served("widget-routes");
        std::fs::create_dir_all(dir.join("docs/widgets")).unwrap();
        std::fs::write(
            dir.join("docs/devpage.yml"),
            "kind: site\ntitle: Docs\nroot: README.md\nwidgets: widgets\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("docs/widgets/Card.tsx"),
            "export default () => <Callout>x</Callout>;",
        )
        .unwrap();
        std::fs::write(dir.join("docs/widgets/Broken.tsx"), "export default (").unwrap();
        let base = serve(&dir, "docs").await;
        let (_, page) = get(&format!("{base}/docs/README.md")).await;
        let card = page
            .split("\"Card\":\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap()
            .to_string();
        let tabs = crate::widgets::system_address("Tabs", "");
        for (address, cache) in [
            (card.as_str(), "public, max-age=31536000, immutable"),
            ("/__devpage/widgets/docs/widgets/Card.tsx", "no-cache"),
            (tabs.as_str(), "public, max-age=31536000, immutable"),
            ("/__devpage/system-widgets/Tabs.tsx", "no-cache"),
            // A module that throws why is never kept.
            ("/__devpage/widgets/docs/widgets/Broken.tsx", "no-cache"),
        ] {
            let (status, headers, body) = fetch(&format!("{base}{address}"), &[]).await;
            assert_eq!(status, 200, "{address}");
            assert_eq!(headers["content-type"], "text/javascript", "{address}");
            assert_eq!(headers["cache-control"], cache, "{address}");
            assert!(!body.is_empty(), "{address}");
        }
        for missing in [
            "/__devpage/system-widgets/Nope.tsx",
            "/__devpage/widgets/../secret.md",
            "/__devpage/widgets/docs/widgets/Gone.tsx",
        ] {
            assert_eq!(get(&format!("{base}{missing}")).await.0, 404, "{missing}");
        }
    }

    #[tokio::test]
    async fn a_page_names_its_widgets_and_carries_their_placeholders() {
        let dir = served("widget-page");
        std::fs::write(
            dir.join("docs/README.md"),
            "# Home\n\n<Callout kind=\"tip\">\nSome **bold**\n</Callout>\n\n```md\n<Callout>code</Callout>\n```\n",
        )
        .unwrap();
        let base = serve(&dir, "docs").await;
        let (_, page) = get(&format!("{base}/docs/README.md")).await;
        assert!(page.contains("<script type=\"importmap\">"), "{page}");
        assert!(
            page.contains(&format!(
                "\"Callout\":\"{}\"",
                crate::widgets::system_address("Callout", "")
            )),
            "{page}"
        );
        assert_eq!(
            page.matches("data-widget=\"Callout\"").count(),
            1,
            "code is left alone: {page}"
        );
        assert!(page.contains("<strong>bold</strong>"), "{page}");
    }

    /// `GET url`, with `headers`: the status, the response headers and the
    /// body's bytes, as sent.
    async fn fetch(
        url: &str,
        headers: &[(&str, &str)],
    ) -> (u16, reqwest::header::HeaderMap, Vec<u8>) {
        let mut request = reqwest::Client::new().get(url);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        (status, headers, response.bytes().await.unwrap().to_vec())
    }

    #[tokio::test]
    async fn every_asset_is_immutable_at_its_content_address_and_answers_at_any_other() {
        let dir = served("assets");
        let base = serve(&dir, "docs").await;
        let (_, page) = get(&format!("{base}/docs/README.md")).await;
        for asset in crate::addresses::ASSETS {
            let address = crate::addresses::address(crate::addresses::Owned::Asset(asset.name));
            if !matches!(
                asset.name,
                "design.js"
                    | "design-worker.js"
                    | "design-stream.js"
                    | "design-tiles.js"
                    | "design-canvas.js"
                    | "design-engine.js"
                    | "design-engine.wasm"
                    | "react.js"
            ) {
                assert!(page.contains(&address), "{} not in {page}", asset.name);
            }
            let (status, headers, body) = fetch(&format!("{base}{address}"), &[]).await;
            assert_eq!(status, 200, "{address}");
            assert_eq!(
                headers["cache-control"],
                "public, max-age=31536000, immutable"
            );
            assert_eq!(headers["content-type"], asset.mime);
            assert_eq!(body, asset.identity, "{address}");
            let (stem, ext) = asset.name.split_once('.').unwrap();
            for old in [
                asset.name.to_string(),
                format!("{stem}.0123456789abcdef.{ext}"),
            ] {
                let (status, headers, body) = fetch(&format!("{base}/__devpage/{old}"), &[]).await;
                assert_eq!(status, 200, "{old}");
                assert_eq!(headers["cache-control"], "no-cache", "{old}");
                assert!(headers.contains_key("etag"), "{old}");
                assert_eq!(body, asset.identity, "{old}");
            }
        }
        assert_eq!(
            fetch(&format!("{base}/__devpage/nope.js"), &[]).await.0,
            404
        );
    }

    #[tokio::test]
    async fn an_asset_is_sent_compressed_as_the_browser_accepts() {
        use std::io::Read;
        let dir = served("encodings");
        let base = serve(&dir, "docs").await;
        let react = crate::addresses::address(crate::addresses::Owned::Asset("react.js"));
        let identity = crate::addresses::ASSETS
            .iter()
            .find(|a| a.name == "react.js")
            .unwrap()
            .identity;
        let url = format!("{base}{react}");

        let (_, headers, body) =
            fetch(&url, &[("accept-encoding", "gzip, deflate, br, zstd")]).await;
        assert_eq!(headers["content-encoding"], "br");
        assert_eq!(headers["vary"], "Accept-Encoding");
        let mut decoded = Vec::new();
        brotli::Decompressor::new(&body[..], 4096)
            .read_to_end(&mut decoded)
            .unwrap();
        assert_eq!(decoded, identity);
        let br_tag = headers["etag"].clone();

        let (_, headers, body) = fetch(&url, &[("accept-encoding", "gzip")]).await;
        assert_eq!(headers["content-encoding"], "gzip");
        let mut decoded = Vec::new();
        flate2::read::GzDecoder::new(&body[..])
            .read_to_end(&mut decoded)
            .unwrap();
        assert_eq!(decoded, identity);
        assert_ne!(
            headers["etag"], br_tag,
            "each encoding is its own representation"
        );

        let (_, headers, body) = fetch(&url, &[]).await;
        assert!(!headers.contains_key("content-encoding"));
        assert_eq!(body, identity);

        let tag = br_tag.to_str().unwrap();
        let (status, headers, body) =
            fetch(&url, &[("accept-encoding", "br"), ("if-none-match", tag)]).await;
        assert_eq!(status, 304);
        assert!(body.is_empty());
        assert_eq!(headers["etag"], tag);
    }

    #[tokio::test]
    async fn a_page_and_an_image_answer_304_when_unchanged() {
        let dir = served("validators");
        std::fs::write(dir.join("docs/logo.png"), b"\x89PNG not really").unwrap();
        let base = serve(&dir, "docs").await;
        for path in ["/docs/README.md", "/docs/logo.png", "/docs/devpage.yml"] {
            let url = format!("{base}{path}");
            let (status, headers, _) = fetch(&url, &[]).await;
            assert_eq!(status, 200, "{path}");
            assert_eq!(headers["cache-control"], "no-cache", "{path}");
            let tag = headers["etag"].to_str().unwrap().to_string();
            assert!(tag.starts_with('"'), "a strong ETag: {tag}");
            let (status, headers, body) = fetch(&url, &[("if-none-match", &tag)]).await;
            assert_eq!(status, 304, "{path}");
            assert!(body.is_empty(), "{path}");
            assert_eq!(headers["etag"], tag.as_str());
            // A proxy that weakened it still matches.
            let weak = format!("W/{tag}");
            assert_eq!(fetch(&url, &[("if-none-match", &weak)]).await.0, 304);
        }
        let url = format!("{base}/docs/README.md");
        let tag = fetch(&url, &[]).await.1["etag"]
            .to_str()
            .unwrap()
            .to_string();
        std::fs::write(dir.join("docs/README.md"), "# Home, edited\n").unwrap();
        let (status, headers, _) = fetch(&url, &[("if-none-match", &tag)]).await;
        assert_eq!(status, 200);
        assert_ne!(headers["etag"], tag.as_str());
        // The streams carry no validator and are never held back.
        let events = reqwest::get(format!("{base}/__devpage/events"))
            .await
            .unwrap();
        assert_eq!(events.status(), 200);
        assert!(!events.headers().contains_key("etag"));
    }

    #[tokio::test]
    async fn a_pages_event_stream_carries_each_change_on_its_bus_in_order() {
        use crate::changes::{Change, Changes};
        let root = std::env::temp_dir().join(format!("devpage-events-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.md"), "# A\n").unwrap();
        let document = Arc::new(
            Document::load(&root.join("a.md"), &ServedFolder::new(&root).unwrap()).unwrap(),
        );
        let changes = Changes::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let router = router(App::testing(document, changes.clone()));
        tokio::spawn(async move { axum::serve(listener, router).await.ok() });
        let mut events = reqwest::get(format!("{address}/__devpage/events"))
            .await
            .unwrap();
        // The stream is open once its headers arrive; give the handler a
        // moment to subscribe before anything is published.
        tokio::time::sleep(Duration::from_millis(100)).await;

        changes.publish(Change::Comments("/a.md".into()));
        changes.publish(Change::Refresh);
        let mut heard = String::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while heard.matches("data:").count() < 2 {
            let chunk = tokio::time::timeout_at(deadline, events.chunk())
                .await
                .expect("two events")
                .unwrap()
                .expect("an open stream");
            heard.push_str(&String::from_utf8_lossy(&chunk));
        }
        let data: Vec<&str> = heard
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim)
            .collect();
        assert_eq!(data, ["comments /a.md", "change"]);
    }
}
