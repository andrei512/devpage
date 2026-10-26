//! The HTTP server: pages rendered on request, the page assets, and a stream
//! that tells open pages a watched file changed.

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
use tokio::sync::broadcast;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use crate::comments::{Anchor, Store};
use crate::designs::Designs;
use crate::document::{self, Document, Entry, Kind};
use crate::index::Index;
use crate::render;

#[derive(Clone)]
struct AppState {
    entry: Arc<Entry>,
    index: Arc<Index>,
    changes: broadcast::Sender<String>,
    store: Arc<Store>,
    identity: Arc<Identity>,
    designs: Designs,
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

/// The server for one entry document, with a fresh index of its served folder.
#[cfg(test)]
pub fn router(
    document: Arc<Document>,
    changes: broadcast::Sender<String>,
    identity: Identity,
) -> Router {
    let index = Arc::new(Index::build(&document.served));
    let designs = Designs::new(document.served.clone());
    router_with_index(
        Arc::new(Entry::new(document)),
        index,
        designs,
        changes,
        identity,
    )
}

/// The server, sharing the entry document and the index the watcher keeps
/// current, logging every request.
pub fn router_with_index(
    entry: Arc<Entry>,
    index: Arc<Index>,
    designs: Designs,
    changes: broadcast::Sender<String>,
    identity: Identity,
) -> Router {
    quiet(entry, index, designs, changes, identity).layer(middleware::from_fn(log))
}

/// The server for `devpage pdf`, which serves only Chrome and logs nothing.
pub fn printing(document: Arc<Document>, changes: broadcast::Sender<String>) -> Router {
    let index = Arc::new(Index::build(&document.served));
    let designs = Designs::new(document.served.clone());
    quiet(
        Arc::new(Entry::new(document)),
        index,
        designs,
        changes,
        Identity::Local(String::new()),
    )
}

fn quiet(
    entry: Arc<Entry>,
    index: Arc<Index>,
    designs: Designs,
    changes: broadcast::Sender<String>,
    identity: Identity,
) -> Router {
    Router::new()
        .route("/", get(home))
        .route("/__devpage/events", get(events))
        .route(
            "/__devpage/comments",
            get(list_comments).post(create_comment),
        )
        .route("/__devpage/comments/reply", post(reply_comment))
        .route("/__devpage/comments/resolve", post(resolve_comment))
        .route("/__devpage/system-widgets/{name}", get(system_widget))
        .route("/__devpage/widgets/{*path}", get(project_widget))
        .route("/__devpage/{asset}", get(asset))
        .route("/{*path}", get(file))
        .route_service(
            "/devpage.design.v1.Designs/Watch",
            tower::ServiceBuilder::new()
                .layer(tonic_web::GrpcWebLayer::new())
                .service(devpage_design::proto::designs_server::DesignsServer::new(
                    crate::designs::Watch {
                        designs: designs.clone(),
                    },
                )),
        )
        .with_state(AppState {
            designs,
            store: Arc::new(Store::new(&entry.document().served)),
            identity: Arc::new(identity),
            entry,
            index,
            changes,
        })
}

/// Logs each request with its status and how long the response took to start.
async fn log(request: Request, next: Next) -> Response {
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

/// `/` leads to the entry document as its file says now — or, while the file
/// is invalid, shows why.
async fn home(State(state): State<AppState>) -> Response {
    let entry = &state.entry;
    match entry.error() {
        Some(error) => {
            Html(render::invalid(&entry.document(), entry.file(), &error)).into_response()
        }
        None => Redirect::temporary(&entry.document().entry()).into_response(),
    }
}

/// A document file opens as its kind; a Markdown file becomes a page of its
/// nearest site, or a doc of one chapter; anything else is served as it is.
async fn file(
    State(state): State<AppState>,
    Path(path): Path<String>,
    RawQuery(query): RawQuery,
) -> Response {
    let print = query.is_some_and(|query| query.split('&').any(|pair| pair == "print"));
    // A design's PNG — unless a real file of that name is there, which is
    // served as it is.
    let real = state
        .entry
        .document()
        .file(&path)
        .is_some_and(|f| f.is_file());
    if let Some(design) = path
        .strip_suffix(".png")
        .filter(|p| p.ends_with(".design") && !real)
    {
        // A design that isn't a file in the served folder is like any other
        // path that isn't: 404, never its problems.
        if !state
            .entry
            .document()
            .file(design)
            .is_some_and(|f| f.is_file())
        {
            return (
                StatusCode::NOT_FOUND,
                format!("{path} is not part of the served folder"),
            )
                .into_response();
        }
        return match state.designs.png(design.to_string()).await {
            Ok(png) => ([(header::CONTENT_TYPE, "image/png")], png).into_response(),
            Err(problems) => (StatusCode::UNPROCESSABLE_ENTITY, problems).into_response(),
        };
    }
    let entry = state.entry.document();
    if path.ends_with(".design") && entry.file(&path).is_some_and(|f| f.is_file()) {
        return Html(crate::designs::page(&path)).into_response();
    }
    let Some(file) = entry.file(&path).filter(|file| file.is_file()) else {
        return (
            StatusCode::NOT_FOUND,
            format!("{path} is not part of the served folder"),
        )
            .into_response();
    };
    let served = &entry.served;
    if document::kind_of(&file).is_some() {
        return match Document::load(&file, served) {
            Ok(document) => show(&document, print),
            Err(error) => {
                Html(render::invalid(&entry, &file, &format!("{error:#}"))).into_response()
            }
        };
    }
    if file.extension().is_some_and(|ext| ext == "md") {
        let source = match tokio::fs::read_to_string(&file).await {
            Ok(source) => source,
            Err(error) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
            }
        };
        // A site is never printed: in print mode a Markdown file is a doc of
        // one chapter, whatever site holds it — what `devpage pdf` prints.
        // A page of a site whose file is invalid shows why, until it is fixed.
        let site = state
            .index
            .site_for(&file)
            .filter(|_| !print)
            .map(|site| (Document::load(&site, served), site));
        return match site {
            Some((Ok(site), _)) => Html(render::page(&site, &file, &source)).into_response(),
            Some((Err(error), site)) => {
                Html(render::invalid(&entry, &site, &format!("{error:#}"))).into_response()
            }
            None => match Document::load(&file, served) {
                Ok(mut doc) => {
                    // A page keeps its site's widgets when printed on its own.
                    if let Some(Ok(site)) = state
                        .index
                        .site_for(&file)
                        .map(|site| Document::load(&site, served))
                    {
                        doc.widgets = site.widgets;
                    }
                    show(&doc, print)
                }
                Err(error) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}")).into_response()
                }
            },
        };
    }
    match tokio::fs::read(&file).await {
        Ok(bytes) => {
            let mime = mime_guess::from_path(&file).first_or_octet_stream();
            ([(header::CONTENT_TYPE, mime.to_string())], bytes).into_response()
        }
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

/// A document as its kind: a site opens on its root page.
fn show(document: &Document, print: bool) -> Response {
    match &document.kind {
        Kind::Site { .. } => Redirect::temporary(&document.entry()).into_response(),
        Kind::Doc { chapters, pdf } => {
            Html(render::doc(document, chapters, pdf, print)).into_response()
        }
        Kind::Slides { slides } => Html(render::slides(document, slides, print)).into_response(),
        Kind::Design => Redirect::temporary(&document.entry()).into_response(),
    }
}

/// Server-sent events: one "change" message per change to a watched file.
async fn events(
    State(state): State<AppState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let stream = BroadcastStream::new(state.changes.subscribe())
        .map(|message| Ok(Event::default().data(message.unwrap_or_else(|_| "change".into()))));
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

/// A system widget, compiled from the TSX built into the binary.
async fn system_widget(Path(name): Path<String>) -> Response {
    let stem = name.trim_end_matches(".tsx");
    // System widgets compose only each other.
    let known: Vec<String> = crate::widgets::SYSTEM
        .iter()
        .map(|(name, _)| name.to_string())
        .collect();
    match crate::widgets::system(stem) {
        Some(source) => compiled(std::path::Path::new(&name), &name, source, &known, None),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// A project widget or a helper it imports, compiled from the served folder.
/// An import without an extension finds the `.ts` or `.tsx` beside it.
async fn project_widget(State(state): State<AppState>, Path(path): Path<String>) -> Response {
    let document = state.entry.document();
    let found = ["", ".tsx", ".ts"]
        .iter()
        .find_map(|ext| document.file(&format!("{path}{ext}")));
    let Some(file) = found else {
        return (
            StatusCode::NOT_FOUND,
            format!("{path} is not part of the served folder"),
        )
            .into_response();
    };
    let folder = file.parent().unwrap_or(&file).to_path_buf();
    let version = crate::widgets::version(&folder);
    match std::fs::read_to_string(&file) {
        Ok(source) => {
            let known = known(&state, &folder);
            // A helper this file imports that doesn't compile: the browser
            // would only say the import failed, so say why, here.
            if let Some(error) = broken_import(&document, &folder, &source, &known, version) {
                return module_error(&error);
            }
            let label = document.url(&file)[1..].to_string();
            compiled(&file, &label, &source, &known, Some(version))
        }
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
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
    crate::widgets::Widgets::of(owner.as_ref().unwrap_or(&entry))
        .modules
        .into_keys()
        .collect()
}

/// The first helper reached through `source`'s relative imports, in `folder`,
/// that doesn't compile: its name and the compiler's message.
fn broken_import(
    document: &crate::document::Document,
    folder: &std::path::Path,
    source: &str,
    known: &[String],
    version: u128,
) -> Option<String> {
    let mut seen = std::collections::HashSet::new();
    let mut queue = relative_imports(source);
    while let Some(import) = queue.pop() {
        let base = folder.join(import.trim_start_matches("./"));
        let Some(file) = ["", ".tsx", ".ts"]
            .iter()
            .map(|ext| std::path::PathBuf::from(format!("{}{ext}", base.display())))
            .find(|path| path.is_file())
        else {
            continue;
        };
        if !seen.insert(file.clone()) {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        match crate::widgets::compile_cached(&file, &text, known, Some(version)) {
            Ok(_) => queue.extend(relative_imports(&text)),
            Err(error) => {
                return Some(format!(
                    "{} does not compile: {error}",
                    &document.url(&file)[1..]
                ));
            }
        }
    }
    None
}

/// The `./…` specifiers a module imports or re-exports from.
fn relative_imports(source: &str) -> Vec<String> {
    ["from \"./", "from './", "import \"./", "import './"]
        .iter()
        .flat_map(|pattern| {
            let quote = &pattern[pattern.len() - 3..pattern.len() - 2];
            source
                .split(pattern)
                .skip(1)
                .filter_map(move |rest| rest.split(quote).next().map(|name| format!("./{name}")))
        })
        .collect()
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

/// A compiled module, or — when it doesn't compile — a module that throws the
/// compiler's message, naming the file as `label`, so the page shows it in the
/// place of the widget it belongs to.
fn compiled(
    path: &std::path::Path,
    label: &str,
    source: &str,
    known: &[String],
    version: Option<u128>,
) -> Response {
    match crate::widgets::compile_cached(path, source, known, version) {
        Ok(js) => (
            [
                (header::CONTENT_TYPE, "text/javascript"),
                (header::CACHE_CONTROL, "no-cache"),
            ],
            js,
        )
            .into_response(),
        Err(error) => module_error(&format!("{label} does not compile: {error}")),
    }
}

/// The page's own files, built into the binary.
async fn asset(Path(name): Path<String>) -> Response {
    let (body, mime): (&'static str, &str) = match name.as_str() {
        "style.css" => (include_str!("../assets/style.css"), "text/css"),
        "devpage.js" => (include_str!("../assets/devpage.js"), "text/javascript"),
        "pele.js" => (include_str!("../assets/vendor/pele.js"), "text/javascript"),
        "react.js" => (include_str!("../assets/vendor/react.js"), "text/javascript"),
        "design.js" => (include_str!("../assets/design.js"), "text/javascript"),
        "design-engine.js" => (
            include_str!("../assets/vendor/design-engine.js"),
            "text/javascript",
        ),
        "design-engine.wasm" => {
            return (
                [
                    (header::CONTENT_TYPE, "application/wasm"),
                    (header::CACHE_CONTROL, "no-cache"),
                ],
                &include_bytes!("../assets/vendor/design-engine.wasm")[..],
            )
                .into_response();
        }
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    (
        [
            (header::CONTENT_TYPE, mime),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        body,
    )
        .into_response()
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

async fn list_comments(State(state): State<AppState>, Query(query): Query<PageQuery>) -> Response {
    match state.store.list(&query.page) {
        Ok(threads) => Json(threads).into_response(),
        Err(error) => failed(error),
    }
}

async fn create_comment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(new): Json<NewThread>,
) -> Response {
    let author = match state.identity.author(&headers) {
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
    match state
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
    let author = match state.identity.author(&headers) {
        Ok(author) => author,
        Err(refused) => return refused,
    };
    match state
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
    if let Err(refused) = state.identity.author(&headers) {
        return refused;
    }
    match state.store.resolve(&reply.page, &reply.id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => failed(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    /// A served folder holding one page, served by the real router on a free
    /// port, with the given identity.
    async fn serve_comments(name: &str, identity: Identity) -> (String, std::path::PathBuf) {
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
        let (changes, _) = broadcast::channel(16);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, router(document, changes, identity))
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
        let (address, root) = serve_comments("life", Identity::Local("Ana".into())).await;
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
        let (address, _) = serve_comments("deployed", identity).await;
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
        let (address, _) = serve_comments("outside", Identity::Local("Ana".into())).await;
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
        let (changes, _) = broadcast::channel(1);
        let router = router(document, changes, Identity::Local("T".into()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await.ok() });
        address
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
    async fn serves_a_folder_without_git_by_what_each_file_is() {
        let dir = served("routes");
        assert!(!dir.join(".git").exists());
        let base = serve(&dir, "docs").await;

        assert_eq!(
            get(&format!("{base}/")).await,
            (307, "/docs/README.md".into())
        );
        let (status, page) = get(&format!("{base}/docs/guide/step.md")).await;
        assert_eq!(status, 200);
        assert!(
            page.contains("<nav id=\"devpage-nav\">"),
            "a page of its nearest site"
        );
        assert!(page.contains("Step · Docs"), "{page}");

        let (status, deck) = get(&format!("{base}/notes/talk.yml")).await;
        assert_eq!(status, 200);
        assert!(deck.contains("Talk"), "a document file opens as its kind");
        assert!(!deck.contains("<nav id=\"devpage-nav\">"));

        let (status, loose) = get(&format!("{base}/notes/loose.md")).await;
        assert_eq!(status, 200);
        assert!(
            !loose.contains("<nav id=\"devpage-nav\">"),
            "in no site: a doc of one chapter"
        );
        assert!(loose.contains("Loose"));

        let (status, broken) = get(&format!("{base}/notes/broken.yml")).await;
        assert_eq!(status, 200);
        assert!(broken.contains("not a valid document"), "{broken}");
        assert!(
            broken.contains("chapter gone.md does not exist"),
            "{broken}"
        );

        assert_eq!(
            get(&format!("{base}/notes/ci.yml")).await,
            (200, "on: push\n".into())
        );
    }

    #[tokio::test]
    async fn a_page_of_a_site_prints_as_a_doc_of_one_chapter() {
        let dir = served("print-page");
        let base = serve(&dir, "docs").await;
        let (status, page) = get(&format!("{base}/docs/guide/step.md?print")).await;
        assert_eq!(status, 200);
        assert!(page.contains("<body class=\"doc print\">"), "{page}");
        assert!(page.contains("@page { size: A4;"), "{page}");
        assert!(!page.contains("<nav id=\"devpage-nav\">"), "{page}");
        let (_, on_screen) = get(&format!("{base}/docs/guide/step.md")).await;
        assert!(
            on_screen.contains("<nav id=\"devpage-nav\">"),
            "still a site page on screen"
        );
    }

    #[tokio::test]
    async fn a_sites_title_links_to_its_own_root_page() {
        let dir = served("titles");
        std::fs::create_dir_all(dir.join("other")).unwrap();
        std::fs::write(dir.join("other/index.md"), "# Index\n").unwrap();
        std::fs::write(dir.join("other/page.md"), "# Page\n").unwrap();
        std::fs::write(
            dir.join("other/devpage.yml"),
            "kind: site\ntitle: Other\nroot: index.md\n",
        )
        .unwrap();
        let base = serve(&dir, "docs").await;
        let (_, page) = get(&format!("{base}/other/page.md")).await;
        assert!(
            page.contains("<a class=\"document-title\" href=\"/other/index.md\">Other</a>"),
            "{page}"
        );
        let (_, home) = get(&format!("{base}/docs/guide/step.md")).await;
        assert!(
            home.contains("<a class=\"document-title\" href=\"/docs/README.md\">Docs</a>"),
            "{home}"
        );
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

    #[tokio::test]
    async fn serves_a_folder_reached_through_a_symbolic_link() {
        let dir = served("symlinked");
        let link = dir.parent().unwrap().join("link");
        std::os::unix::fs::symlink(&dir, &link).unwrap();
        let base = serve(&link, "docs").await;
        let (status, page) = get(&format!("{base}/docs/guide/step.md")).await;
        assert_eq!(status, 200, "{page}");
        assert_eq!(get(&format!("{base}/notes/ci.yml")).await.0, 200);
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
        let entry = Arc::new(Entry::new(document));
        let (changes, _) = broadcast::channel(16);
        let designs = Designs::new(dir.clone());
        let _watcher = crate::watch::start(
            entry.clone(),
            index.clone(),
            designs.clone(),
            changes.clone(),
        )
        .unwrap();
        let router = router_with_index(entry, index, designs, changes, Identity::Local("T".into()));
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

        let version = crate::widgets::version(&dir.join("docs/widgets"));
        let (status, card) = get(&format!(
            "{base}/__devpage/widgets/docs/widgets/Card.tsx?v={version}"
        ))
        .await;
        assert_eq!(status, 200);
        assert!(card.contains(&format!("\"./label?v={version}\"")), "{card}");
        assert!(card.contains("__devpageUses = [\"Callout\"]"), "{card}");
        let (status, helper) = get(&format!(
            "{base}/__devpage/widgets/docs/widgets/label?v={version}"
        ))
        .await;
        assert_eq!(status, 200, "an import without an extension finds the .ts");
        assert!(helper.contains("label"), "{helper}");

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
            page.contains("\"Callout\":\"/__devpage/system-widgets/Callout.tsx\""),
            "{page}"
        );
        assert_eq!(
            page.matches("data-widget=\"Callout\"").count(),
            1,
            "code is left alone: {page}"
        );
        assert!(page.contains("<strong>bold</strong>"), "{page}");
    }
}
