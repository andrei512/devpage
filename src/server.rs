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
use crate::document::{self, Document, Kind};
use crate::index::Index;
use crate::render;

#[derive(Clone)]
struct AppState {
    document: Arc<Document>,
    index: Arc<Index>,
    changes: broadcast::Sender<String>,
    store: Arc<Store>,
    identity: Arc<Identity>,
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
pub fn router(
    document: Arc<Document>,
    changes: broadcast::Sender<String>,
    identity: Identity,
) -> Router {
    let index = Arc::new(Index::build(&document.served));
    router_with_index(document, index, changes, identity)
}

/// The server, sharing an index the watcher keeps current.
pub fn router_with_index(
    document: Arc<Document>,
    index: Arc<Index>,
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
        .route("/__devpage/{asset}", get(asset))
        .route("/{*path}", get(file))
        .layer(middleware::from_fn(log))
        .with_state(AppState {
            store: Arc::new(Store::new(&document.served)),
            identity: Arc::new(identity),
            document,
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

async fn home(State(state): State<AppState>) -> Redirect {
    Redirect::temporary(&state.document.entry())
}

/// A document file opens as its kind; a Markdown file becomes a page of its
/// nearest site, or a doc of one chapter; anything else is served as it is.
async fn file(
    State(state): State<AppState>,
    Path(path): Path<String>,
    RawQuery(query): RawQuery,
) -> Response {
    let print = query.is_some_and(|query| query.split('&').any(|pair| pair == "print"));
    let Some(file) = state.document.file(&path).filter(|file| file.is_file()) else {
        return (
            StatusCode::NOT_FOUND,
            format!("{path} is not part of the served folder"),
        )
            .into_response();
    };
    let served = &state.document.served;
    if document::kind_of(&file).is_some() {
        return match Document::load(&file, served) {
            Ok(document) => show(&document, print),
            Err(error) => Html(render::invalid(&state.document, &file, &error)).into_response(),
        };
    }
    if file.extension().is_some_and(|ext| ext == "md") {
        let Ok(source) = tokio::fs::read_to_string(&file).await else {
            return (StatusCode::INTERNAL_SERVER_ERROR, "unreadable").into_response();
        };
        let site = state
            .index
            .site_for(&file)
            .and_then(|site| Document::load(&site, served).ok());
        return match site {
            Some(site) => Html(render::page(&site, &file, &source)).into_response(),
            None => match Document::load(&file, served) {
                Ok(doc) => show(&doc, print),
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

/// The page's own files, built into the binary.
async fn asset(Path(name): Path<String>) -> Response {
    let (body, mime): (&'static str, &str) = match name.as_str() {
        "style.css" => (include_str!("../assets/style.css"), "text/css"),
        "devpage.js" => (include_str!("../assets/devpage.js"), "text/javascript"),
        "pele.js" => (include_str!("../assets/vendor/pele.js"), "text/javascript"),
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
            page.contains("<nav id=\"nav\">"),
            "a page of its nearest site"
        );
        assert!(page.contains("Step · Docs"), "{page}");

        let (status, deck) = get(&format!("{base}/notes/talk.yml")).await;
        assert_eq!(status, 200);
        assert!(deck.contains("Talk"), "a document file opens as its kind");
        assert!(!deck.contains("<nav id=\"nav\">"));

        let (status, loose) = get(&format!("{base}/notes/loose.md")).await;
        assert_eq!(status, 200);
        assert!(
            !loose.contains("<nav id=\"nav\">"),
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
}
