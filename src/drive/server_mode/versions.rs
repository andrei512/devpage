//! Serving a version: the request handed to that version's own router, made
//! on a cache miss from its zip in the bucket.

use std::sync::Arc;
use std::time::SystemTime;

use anyhow::Result;
use axum::Router;
use axum::body::Body;
use axum::extract::{Path as UrlPath, Request, State};
use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use devpage_core::cache::Entry;
use devpage_core::document::Document;
use tower::ServiceExt;

use super::ServerMode;
use super::api::unpack;
use super::cache::{evict, size_of};
use crate::changes::Changes;
use crate::server;

/// `/d/<id>`: the latest version's front page.
pub async fn latest(
    State(state): State<ServerMode>,
    UrlPath(id): UrlPath<String>,
    request: Request,
) -> Response {
    match state.store.latest(&id) {
        Ok(Some(n)) => serve(&state, &id, n, "/", request).await,
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

pub async fn version_root(
    State(state): State<ServerMode>,
    UrlPath((id, n)): UrlPath<(String, u64)>,
    request: Request,
) -> Response {
    serve(&state, &id, n, "/", request).await
}

pub async fn version(
    State(state): State<ServerMode>,
    UrlPath((id, n, rest)): UrlPath<(String, u64, String)>,
    request: Request,
) -> Response {
    serve(&state, &id, n, &format!("/{rest}"), request).await
}

/// PROTOTYPE: a page's script and widgets ask absolute addresses —
/// `/__devpage/events`, widgets, comments, the design stream — so each is
/// sent to the version the asking page is.
pub async fn by_referer(State(state): State<ServerMode>, request: Request) -> Response {
    let from = request
        .headers()
        .get(axum::http::header::REFERER)
        .and_then(|r| r.to_str().ok())
        .and_then(|r| r.split_once("/d/").map(|(_, rest)| rest.to_string()));
    let Some(from) = from else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut parts = from.split('/');
    let (Some(id), Some("v"), Some(n)) = (parts.next(), parts.next(), parts.next()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(n) = n.parse::<u64>() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let (id, path) = (id.to_string(), request.uri().path().to_string());
    serve(&state, &id, n, &path, request).await
}

/// The request answered by version `n`'s router, as if for `path`.
async fn serve(state: &ServerMode, id: &str, n: u64, path: &str, request: Request) -> Response {
    match state.store.has(id, n) {
        Ok(true) => {}
        Ok(false) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
        }
    }
    let router = match version_router(state, id, n).await {
        Ok(router) => router,
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}")).into_response();
        }
    };
    let (mut parts, body) = request.into_parts();
    let query = parts
        .uri
        .query()
        .map(|q| format!("?{q}"))
        .unwrap_or_default();
    parts.uri = format!("{path}{query}")
        .parse::<Uri>()
        .unwrap_or_else(|_| Uri::from_static("/"));
    // A fresh request: the outer router's path parameters ride in the
    // extensions, and the version's router would read them as its own.
    parts.extensions = Default::default();
    router
        .oneshot(Request::from_parts(parts, Body::new(body)))
        .await
        .into_response()
}

/// Version `n`'s router: kept while it is in the cache, made on a miss from
/// the zip in the bucket.
async fn version_router(state: &ServerMode, id: &str, n: u64) -> Result<Router> {
    let key = (id.to_string(), n);
    let cache = &state.cache;
    if let Some(router) = cache.routers.lock().unwrap().get(&key).cloned() {
        if let Some(entry) = cache.entries.lock().unwrap().get_mut(&key) {
            entry.read = SystemTime::now();
        }
        return Ok(router);
    }
    let zip = state.bucket.get_version(id, n).await?;
    let dir = cache.dir.join(id).join(n.to_string());
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    unpack(&zip, &dir)?;
    let dir = dir.canonicalize()?;
    let mount = format!("/d/{id}/v/{n}");
    devpage_core::served::ServedFolder::mount_at(&dir, &mount);
    let document = Document::load(&dir, &dir)?;
    let mut options = server::Options::new(Arc::new(document), Changes::default());
    options.comments = None;
    // The served folder carries the mount, so every address the version's
    // router makes — redirects included — is already under it.
    let router = server::router(options);
    cache
        .routers
        .lock()
        .unwrap()
        .insert(key.clone(), router.clone());
    cache.entries.lock().unwrap().insert(
        key.clone(),
        Entry {
            document: id.to_string(),
            version: n,
            bytes: size_of(&dir),
            read: SystemTime::now(),
        },
    );
    evict(cache, &key);
    Ok(router)
}
