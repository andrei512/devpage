//! The HTTP server: pages rendered on request, the page assets, and a stream
//! that tells open pages a watched file changed.

mod canvas;
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

use crate::changes::{Changes, Scope};
use crate::check::Preview;
use crate::comments::{Anchor, Store};
use crate::designs::Designs;
use crate::document::{Document, Entry};
use crate::external::External;
use crate::index::Index;
use crate::resolve::{Answer, Resolver};
use crate::slide_png::Shots;

#[derive(Clone)]
struct AppState {
    entry: Arc<Entry>,
    index: Arc<Index>,
    changes: Changes,
    /// `None` when comments are off: the pages show none and every comment
    /// route answers 404.
    comments: Option<Arc<Comments>>,
    designs: Designs,
    /// Slides as images: the Chrome that draws them and what it drew.
    shots: Shots,
    /// The problems each page lists above itself.
    preview: Arc<Preview>,
}

/// Comments on: where threads are kept, and who writes them.
struct Comments {
    store: Store,
    identity: Identity,
}

impl AppState {
    /// The comments, or — when they are off — the 404 every comment route
    /// answers, as if it weren't there.
    #[allow(clippy::result_large_err)]
    fn comments(&self) -> Result<&Comments, Response> {
        self.comments
            .as_deref()
            .ok_or_else(|| (StatusCode::NOT_FOUND, "comments are off").into_response())
    }

    fn comments_on(&self) -> bool {
        self.comments.is_some()
    }
}

/// Who writes a comment: one local author, or — deployed — whoever the auth
/// proxy names in a header. devpage authenticates no one itself.
pub enum Identity {
    Local(String),
    Header(String),
}

impl Identity {
    #[allow(clippy::result_large_err)]
    fn author(&self, headers: &HeaderMap) -> Result<String, Response> {
        match self {
            Identity::Local(name) => Ok(name.clone()),
            Identity::Header(name) => headers
                .get(name.as_str())
                .and_then(|value| value.to_str().ok())
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .ok_or_else(|| {
                    (StatusCode::UNAUTHORIZED, format!("no {name} header")).into_response()
                }),
        }
    }
}

/// Everything a server is made of. [`Options::new`] serves one document
/// the way tests and `devpage check` want it; a caller changes the fields it
/// cares about with struct update syntax, so a new dependency never reaches
/// every caller.
pub struct Options {
    /// The entry document, followed as its file changes.
    pub entry: Arc<Entry>,
    /// The documents of the served folder, and its search index if any.
    pub index: Arc<Index>,
    pub designs: Designs,
    /// What tells open pages there is something new to show.
    pub changes: Changes,
    /// Who writes comments, or `None` when they are off (`--no-comments`).
    pub comments: Option<Identity>,
    /// The Chrome that draws slides as PNGs, and what it drew.
    pub shots: Shots,
    /// The problems each page lists.
    pub preview: Arc<Preview>,
    /// Whether every request is logged, as `devpage` does.
    pub log: bool,
}

impl Options {
    /// One document, with a fresh index of its served folder and no search
    /// index, comments on as one local author, nothing logged.
    pub fn new(document: Arc<Document>, changes: Changes) -> Self {
        Self {
            index: Arc::new(Index::build(&document.served)),
            designs: Designs::new(document.served.path().to_path_buf()),
            shots: Shots::new(&changes),
            preview: Arc::new(Preview::new(
                External::http(Some(changes.clone())),
                Arc::new(crate::terminal::Terminal),
            )),
            entry: Arc::new(Entry::new(document)),
            comments: Some(Identity::Local(String::new())),
            changes,
            log: false,
        }
    }

    /// The server for `devpage pdf` and `devpage video`, which serves only
    /// Chrome: nothing logged, and problems announced to no one.
    pub fn printing(document: Arc<Document>, changes: Changes) -> Self {
        Self {
            preview: Arc::new(Preview::new(
                External::http(None),
                Arc::new(crate::terminal::Terminal),
            )),
            ..Self::new(document, changes)
        }
    }
}

/// The server `options` describe.
pub fn router(options: Options) -> Router {
    let Options {
        entry,
        index,
        designs,
        changes,
        comments,
        shots,
        preview,
        log,
    } = options;
    let router = routes(AppState {
        comments: comments.map(|identity| {
            Arc::new(Comments {
                store: Store::new(&entry.document().served),
                identity,
            })
        }),
        designs,
        entry,
        index,
        changes,
        shots,
        preview,
    });
    if log {
        router.layer(middleware::from_fn(log_request))
    } else {
        router
    }
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
                    watch_stream::Watch {
                        designs: designs.clone(),
                    },
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

/// The resolver for a request: the server's documents, mounted at `/`.
fn resolver(state: &AppState) -> Resolver<'_> {
    Resolver {
        entry: &state.entry,
        index: &state.index,
        preview: &state.preview,
        comments: state.comments_on(),
        mount: "",
    }
}

/// `/`: whatever the resolver says the entry document leads to now.
async fn home(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let answer = resolver(&state).home();
    respond(&state, &headers, answer).await
}

/// Any other path: answered as the resolver decides.
async fn file(
    State(state): State<AppState>,
    Path(path): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let answer = resolver(&state).path(&path, query.as_deref());
    respond(&state, &headers, answer).await
}

/// An answer as a response: the file's bytes read, the PNG drawn.
async fn respond(state: &AppState, headers: &HeaderMap, answer: Answer) -> Response {
    match answer {
        Answer::Html(html) => Html(html).into_response(),
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
        Answer::Canvas { design, theme } => Html(canvas::page(&design, theme)).into_response(),
        Answer::DesignPng { design, theme } => design_png(state, headers, design, theme).await,
        Answer::SlidePng { page, n } => slide_png(state, headers, &page, n).await,
    }
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
    let tag = state
        .designs
        .etag(&design, theme)
        .await
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
    match state.designs.png(design, theme).await {
        Ok(png) => {
            let mut response = ([(header::CONTENT_TYPE, "image/png")], png).into_response();
            if let Some(tag) = tag.and_then(|tag| tag.parse().ok()) {
                response.headers_mut().insert(header::ETAG, tag);
            }
            response
        }
        Err(problems) => (StatusCode::UNPROCESSABLE_ENTITY, problems).into_response(),
    }
}

/// Slide `n` of the deck whose page is at `page`, drawn by Chrome. A slide
/// drawn since the last change comes from the cache, and is a 304 for a
/// request that already holds it. Asked for by a page devpage's own Chrome
/// is shooting, it is never drawn: the cache's copy, or 508.
async fn slide_png(state: &AppState, headers: &HeaderMap, page: &str, n: usize) -> Response {
    let shot = if headers.contains_key(crate::pdf::SHOT_HEADER) {
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
        let serving = state.clone();
        match state.shots.png(page, n, move || routes(serving)).await {
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

/// Server-sent events: one message per change the server's documents —
/// mounted at `/` — hear on the change bus.
async fn events(
    State(state): State<AppState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let heard = state.changes.subscribe(Scope::root());
    let stream = futures::stream::unfold(heard, |mut heard| async move {
        let change = heard.next().await?;
        Some((Ok(Event::default().data(change.message())), heard))
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

/// The `v` a widget's address was asked with, if any.
fn asked_version(query: Option<&str>) -> Option<&str> {
    query?.split('&').find_map(|pair| pair.strip_prefix("v="))
}

/// A system widget, compiled from the TSX built into the binary: `immutable`
/// at its current address, `no-cache` at any other.
async fn system_widget(
    State(state): State<AppState>,
    Path(name): Path<String>,
    RawQuery(query): RawQuery,
) -> Response {
    let stem = name.trim_end_matches(".tsx");
    match state.index.modules().system(stem) {
        Some(module) => {
            let current = crate::addresses::system_widget_hash(stem)
                .is_some_and(|hash| asked_version(query.as_deref()) == Some(hash));
            compiled(&name, module, current)
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// A project widget or a helper it imports, compiled from the served folder:
/// `immutable` at its current address, `no-cache` at any other. An import
/// without an extension finds the `.ts` or `.tsx` beside it.
async fn project_widget(
    State(state): State<AppState>,
    Path(path): Path<String>,
    RawQuery(query): RawQuery,
) -> Response {
    let document = state.entry.document();
    let found = ["", ".tsx", ".ts"]
        .iter()
        .find_map(|ext| document.file(&format!("{path}{ext}")))
        .filter(|file| file.is_file());
    let Some(file) = found else {
        return (
            StatusCode::NOT_FOUND,
            format!("{path} is not part of the served folder"),
        )
            .into_response();
    };
    let folder = file.parent().unwrap_or(&file).to_path_buf();
    let known = known(&state, &folder);
    // The version asked about and the bytes sent come from one reading of
    // the files: a save between the two can't be served as `immutable` under
    // the version before it.
    let snapshot = state.index.modules().snapshot(&document.served, &file);
    let current = asked_version(query.as_deref()) == Some(snapshot.hash(&file, &known).as_str());
    // A helper this file imports that doesn't compile: the browser would only
    // say the import failed, so say why, here.
    if let Some(error) = broken_import(&document, &snapshot, &file, &known) {
        return module_error(&error);
    }
    let label = document.url(&file)[1..].to_string();
    compiled(&label, snapshot.compile(&file, &known), current)
}

/// The names a widget in `folder` can render by name: the system widgets and
/// those of the document whose widgets folder it is — the entry document's
/// when no other names it.
fn known(state: &AppState, folder: &std::path::Path) -> Vec<String> {
    let entry = state.entry.document();
    let owner = if entry.widgets.as_deref() == Some(folder) {
        None
    } else {
        state
            .index
            .documents()
            .into_iter()
            .filter_map(|file| crate::document::Document::load(&file, &entry.served).ok())
            .find(|document| document.widgets.as_deref() == Some(folder))
    };
    crate::widgets::Widgets::of(owner.as_ref().unwrap_or(&entry), state.index.modules())
        .modules
        .into_keys()
        .collect()
}

/// The first helper reached through `file`'s relative imports that doesn't
/// compile: its name and the compiler's message.
fn broken_import(
    document: &crate::document::Document,
    snapshot: &crate::widgets::Snapshot,
    file: &std::path::Path,
    known: &[String],
) -> Option<String> {
    snapshot
        .closure(file)
        .into_iter()
        .skip(1)
        .find_map(|(helper, _)| {
            snapshot
                .compile(&helper, known)
                .err()
                .map(|error| format!("{} does not compile: {error}", &document.url(&helper)[1..]))
        })
}

/// A module that throws `message`, so the page shows it in the widget's place.
fn module_error(message: &str) -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/javascript"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        format!(
            "throw new Error({});",
            serde_json::to_string(message).unwrap_or_default()
        ),
    )
        .into_response()
}

/// `Cache-Control` for what answers a current content address: it never
/// changes.
const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// A compiled module — `immutable` when asked for at its `current` address —
/// or, when it doesn't compile, a module that throws the compiler's message,
/// naming the file as `label`, so the page shows it in the place of the
/// widget it belongs to.
fn compiled(label: &str, result: anyhow::Result<String>, current: bool) -> Response {
    match result {
        Ok(js) => (
            [
                (header::CONTENT_TYPE, "text/javascript"),
                (
                    header::CACHE_CONTROL,
                    if current { IMMUTABLE } else { "no-cache" },
                ),
            ],
            js,
        )
            .into_response(),
        Err(error) => module_error(&format!("{label} does not compile: {error}")),
    }
}

/// A built-in asset, from the manifest `build.rs` made: `immutable` at its
/// content address; at its bare name or a stale hash, the current bytes with
/// `no-cache`, so a page from an older build still loads. Sent brotli- or
/// gzip-compressed when the browser takes it.
async fn asset(Path(name): Path<String>, headers: HeaderMap) -> Response {
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

#[derive(Deserialize)]
struct Reply {
    page: String,
    id: String,
    #[serde(default)]
    text: String,
}

fn failed(error: anyhow::Error) -> Response {
    (StatusCode::BAD_REQUEST, format!("{error:#}")).into_response()
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
        .index
        .search()
        .map(|search| search.search(&query.q))
        .unwrap_or_default();
    Json(answer).into_response()
}

async fn list_comments(State(state): State<AppState>, Query(query): Query<PageQuery>) -> Response {
    let comments = match state.comments() {
        Ok(comments) => comments,
        Err(off) => return off,
    };
    match comments.store.list(&query.page) {
        Ok(threads) => Json(threads).into_response(),
        Err(error) => failed(error),
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
    let author = match comments.identity.author(&headers) {
        Ok(author) => author,
        Err(refused) => return refused,
    };
    // A thread is about a file that exists in the served folder.
    if state
        .entry
        .document()
        .file(&new.page)
        .is_none_or(|file| !file.is_file())
    {
        return (
            StatusCode::BAD_REQUEST,
            format!("{} is not a page in the served folder", new.page),
        )
            .into_response();
    }
    match comments
        .store
        .create(&new.page, new.anchor, &author, &new.text)
    {
        Ok(id) => Json(serde_json::json!({ "id": id })).into_response(),
        Err(error) => failed(error),
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
    let author = match comments.identity.author(&headers) {
        Ok(author) => author,
        Err(refused) => return refused,
    };
    match comments
        .store
        .reply(&reply.page, &reply.id, &author, &reply.text)
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => failed(error),
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
    if let Err(refused) = comments.identity.author(&headers) {
        return refused;
    }
    match comments.store.resolve(&reply.page, &reply.id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => failed(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    /// A served folder holding one page, served by the real router on a free
    /// port, with the given comment author — or `None`, comments off.
    async fn serve_comments(
        name: &str,
        comments: Option<Identity>,
    ) -> (String, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("devpage-routes-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("a.md"),
            "# A\n\nThe client retries every write.\n",
        )
        .unwrap();
        let document = Arc::new(Document::load(&root.join("a.md"), &root).unwrap());
        let changes = crate::changes::Changes::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(
                listener,
                router(Options {
                    comments,
                    ..Options::new(document, changes)
                }),
            )
            .await
            .unwrap()
        });
        (address, root)
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
            .json(&json!({ "page": "/a.md", "id": id, "text": "Only the outbox." }))
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
        assert!(
            root.join(format!(".devpage/comments/a.md/{id}.yml"))
                .is_file()
        );

        let resolved = client
            .post(format!("{address}/__devpage/comments/resolve"))
            .json(&json!({ "page": "/a.md", "id": id }))
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
                .json(&json!({ "page": "/a.md", "id": id, "text": "x" }))
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
    async fn a_thread_on_a_page_that_isnt_in_the_served_folder_is_refused() {
        let (address, _) = serve_comments("outside", Some(Identity::Local("Ana".into()))).await;
        let mut thread = new_thread();
        for page in ["/../escape.md", "/missing.md"] {
            thread["page"] = json!(page);
            let refused = reqwest::Client::new()
                .post(format!("{address}/__devpage/comments"))
                .json(&thread)
                .send()
                .await
                .unwrap();
            assert_eq!(refused.status(), 400, "{page}");
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
                .json(&json!({ "page": "/a.md", "id": "x", "text": "hi" }))
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
    fn deployed_authors_come_from_the_proxy_header_or_are_refused() {
        let deployed = Identity::Header("X-Forwarded-Email".into());
        let mut headers = HeaderMap::new();
        let refused = deployed.author(&headers).unwrap_err();
        assert_eq!(refused.status(), StatusCode::UNAUTHORIZED);
        headers.insert("x-forwarded-email", "ana@example.com".parse().unwrap());
        assert_eq!(deployed.author(&headers).unwrap(), "ana@example.com");
        let local = Identity::Local("Andrei".into());
        assert_eq!(local.author(&headers).unwrap(), "Andrei");
    }

    /// Serves `dir` as the served folder, with `entry` as the entry document,
    /// and returns the server's address.
    async fn serve(dir: &std::path::Path, entry: &str) -> String {
        let document = Arc::new(Document::load(&dir.join(entry), dir).unwrap());
        let changes = crate::changes::Changes::default();
        let router = router(Options {
            comments: Some(Identity::Local("T".into())),
            ..Options::new(document, changes)
        });
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
        let document = Arc::new(Document::load(&dir, &dir).unwrap());
        let changes = crate::changes::Changes::default();
        let router = router(Options {
            index: Arc::new(Index::build(&dir).with_search(&dir.join("devpage.yml"))),
            comments: None,
            preview: crate::check::testing::preview(),
            ..Options::new(document, changes)
        });
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

        let document = Arc::new(Document::load(&site, &dir).unwrap());
        let index = Arc::new(Index::build(&dir));
        let entry = Arc::new(Entry::new(document.clone()));
        let changes = crate::changes::Changes::default();
        let designs = Designs::new(dir.clone());
        let _watcher = crate::watch::start(
            entry.clone(),
            index.clone(),
            designs.clone(),
            changes.clone(),
        )
        .unwrap();
        let router = router(Options {
            entry,
            index,
            designs,
            comments: Some(Identity::Local("T".into())),
            preview: crate::check::testing::preview(),
            ..Options::new(document, changes.clone())
        });
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
    async fn widgets_are_served_compiled_for_the_document_they_belong_to() {
        let dir = served("widget-routes");
        std::fs::create_dir_all(dir.join("docs/widgets")).unwrap();
        std::fs::create_dir_all(dir.join("notes/parts")).unwrap();
        std::fs::write(
            dir.join("docs/devpage.yml"),
            "kind: site\ntitle: Docs\nroot: README.md\nwidgets: widgets\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("docs/widgets/Card.tsx"),
            "import { label } from \"./label\";\nexport default () => <Callout>{label}</Callout>;",
        )
        .unwrap();
        std::fs::write(
            dir.join("docs/widgets/label.ts"),
            "export const label = 'x';",
        )
        .unwrap();
        std::fs::write(dir.join("docs/widgets/Broken.tsx"), "export default (").unwrap();
        // Another document's widgets compose with that document's names.
        std::fs::write(
            dir.join("notes/talk.yml"),
            "kind: slides\ntitle: Talk\nslides: [one.md]\nwidgets: parts\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("notes/parts/Pill.tsx"),
            "export default () => <i>p</i>;",
        )
        .unwrap();
        std::fs::write(
            dir.join("notes/parts/Row.tsx"),
            "export default () => <Pill />;",
        )
        .unwrap();
        let base = serve(&dir, "docs").await;

        let (status, system) = get(&format!("{base}/__devpage/system-widgets/Tabs.tsx")).await;
        assert_eq!(status, 200);
        assert!(system.contains("export const __devpageUses"), "{system}");
        assert_eq!(
            get(&format!("{base}/__devpage/system-widgets/Nope.tsx"))
                .await
                .0,
            404
        );

        let (_, page) = get(&format!("{base}/docs/README.md")).await;
        let card_address = page
            .split("\"Card\":\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap()
            .to_string();
        let served = dir.canonicalize().unwrap();
        let docs = Document::load(&served.join("docs"), &served).unwrap();
        let known: Vec<String> = crate::widgets::Widgets::of(&docs, &Default::default())
            .modules
            .into_keys()
            .collect();
        let address = |file: &str| {
            crate::addresses::address(crate::addresses::Owned::Module {
                snapshot: &crate::widgets::Modules::default().snapshot(&served, &served.join(file)),
                file: &served.join(file),
                known: &known,
            })
        };
        assert_eq!(card_address, address("docs/widgets/Card.tsx"));
        let response = reqwest::get(format!("{base}{card_address}")).await.unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.headers()["cache-control"],
            "public, max-age=31536000, immutable"
        );
        let card = response.text().await.unwrap();
        // The helper is imported by its own address, whatever the import says.
        let label = address("docs/widgets/label.ts");
        assert!(card.contains(&format!("\"{label}\"")), "{card}");
        assert!(card.contains("__devpageUses = [\"Callout\"]"), "{card}");
        let response = reqwest::get(format!("{base}{label}")).await.unwrap();
        assert_eq!(
            response.headers()["cache-control"],
            "public, max-age=31536000, immutable"
        );
        assert!(response.text().await.unwrap().contains("label"));
        // Any other version answers with the current module, never immutable.
        for stale in [
            "/__devpage/widgets/docs/widgets/Card.tsx?v=0123456789abcdef",
            "/__devpage/widgets/docs/widgets/Card.tsx",
        ] {
            let response = reqwest::get(format!("{base}{stale}")).await.unwrap();
            assert_eq!(response.headers()["cache-control"], "no-cache", "{stale}");
            assert_eq!(response.text().await.unwrap(), card, "{stale}");
        }
        let (status, helper) = get(&format!("{base}/__devpage/widgets/docs/widgets/label")).await;
        assert_eq!(status, 200, "an import without an extension finds the .ts");
        assert!(helper.contains("label"), "{helper}");
        let system = crate::addresses::address(crate::addresses::Owned::SystemWidget("Tabs"));
        let response = reqwest::get(format!("{base}{system}")).await.unwrap();
        assert_eq!(
            response.headers()["cache-control"],
            "public, max-age=31536000, immutable"
        );

        let (_, broken) = get(&format!("{base}/__devpage/widgets/docs/widgets/Broken.tsx")).await;
        assert!(broken.starts_with("throw new Error("), "{broken}");

        let (_, row) = get(&format!("{base}/__devpage/widgets/notes/parts/Row.tsx")).await;
        assert!(row.contains("__devpageUses = [\"Pill\"]"), "{row}");

        let (status, _) = get(&format!("{base}/__devpage/widgets/../secret.md")).await;
        assert_eq!(status, 404);
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
                crate::addresses::address(crate::addresses::Owned::SystemWidget("Callout"))
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
                "design.js" | "design-engine.js" | "design-engine.wasm" | "react.js"
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
    async fn a_pages_event_stream_carries_only_its_documents_changes() {
        use crate::changes::{Change, Changes, Scope};
        let root = std::env::temp_dir().join(format!("devpage-events-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.md"), "# A\n").unwrap();
        let document = Arc::new(Document::load(&root.join("a.md"), &root).unwrap());
        let changes = Changes::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let router = router(Options {
            preview: crate::check::testing::preview(),
            ..Options::new(document, changes.clone())
        });
        tokio::spawn(async move { axum::serve(listener, router).await.ok() });
        let mut events = reqwest::get(format!("{address}/__devpage/events"))
            .await
            .unwrap();
        // The stream is open once its headers arrive; give the handler a
        // moment to subscribe before anything is published.
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Another mount's change never reaches it; its own, and one for
        // every document, do — in order.
        changes.publish(&Scope::Mount("/d/other".into()), Change::Refresh);
        changes.publish(&Scope::root(), Change::Comments("/a.md".into()));
        changes.publish(&Scope::Every, Change::Refresh);
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
