//! The HTTP server: pages rendered on request, the page assets, and a stream
//! that tells open pages a watched file changed.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Json;
use axum::Router;
use axum::extract::Query;
use axum::extract::{Path, RawQuery, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use serde::Deserialize;
use tokio::sync::broadcast;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use crate::comments::{self, Anchor, Store};
use crate::document::{Document, Kind};
use crate::render;

#[derive(Clone)]
struct AppState {
    document: Arc<Document>,
    changes: broadcast::Sender<String>,
    store: Arc<Store>,
    author: Arc<String>,
}

pub fn router(document: Arc<Document>, changes: broadcast::Sender<String>) -> Router {
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
            store: Arc::new(Store::new(&document.repo)),
            author: Arc::new(comments::local_author(&document.repo)),
            document,
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

/// A Markdown file becomes a page; anything else in the folder is served as is.
async fn file(
    State(state): State<AppState>,
    Path(path): Path<String>,
    RawQuery(query): RawQuery,
) -> Response {
    let print = query.is_some_and(|query| query.split('&').any(|pair| pair == "print"));
    let itself = state.document.file(&path) == state.document.path().canonicalize().ok();
    match &state.document.kind {
        Kind::Doc { chapters, pdf } if itself => {
            return Html(render::doc(&state.document, chapters, pdf, print)).into_response();
        }
        Kind::Slides { slides } if itself => {
            return Html(render::slides(&state.document, slides, print)).into_response();
        }
        _ => {}
    }
    let Some(file) = state.document.file(&path).filter(|file| file.is_file()) else {
        return (
            StatusCode::NOT_FOUND,
            format!("{path} is not part of this repository"),
        )
            .into_response();
    };
    if file.extension().is_some_and(|ext| ext == "md") {
        match tokio::fs::read_to_string(&file).await {
            Ok(source) => Html(render::page(&state.document, &file, &source)).into_response(),
            Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
        }
    } else {
        match tokio::fs::read(&file).await {
            Ok(bytes) => {
                let mime = mime_guess::from_path(&file).first_or_octet_stream();
                ([(header::CONTENT_TYPE, mime.to_string())], bytes).into_response()
            }
            Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
        }
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

async fn create_comment(State(state): State<AppState>, Json(new): Json<NewThread>) -> Response {
    match state
        .store
        .create(&new.page, new.anchor, &state.author, &new.text)
    {
        Ok(id) => Json(serde_json::json!({ "id": id })).into_response(),
        Err(error) => failed(error),
    }
}

async fn reply_comment(State(state): State<AppState>, Json(reply): Json<Reply>) -> Response {
    match state
        .store
        .reply(&reply.page, &reply.id, &state.author, &reply.text)
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => failed(error),
    }
}

async fn resolve_comment(State(state): State<AppState>, Json(reply): Json<Reply>) -> Response {
    match state.store.resolve(&reply.page, &reply.id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => failed(error),
    }
}
