//! Serving a version: the request handed to that version's own router under
//! its mount — the latest at `/d/<id>`, every version at `/d/<id>/v/<n>` —
//! made on a cache miss from its zip in the bucket.

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use axum::body::Body;
use axum::extract::{Path as UrlPath, Request, State};
use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use devpage_core::check::Preview;
use devpage_core::document::Document;
use devpage_core::index::Index;
use devpage_core::search::Search;
use devpage_core::served::ServedFolder;
use tower::ServiceExt;

use super::ServerMode;
use super::cache::Served;
use super::metrics::{self, Stage};
use crate::adapters::sqlite::DocumentThreads;
use crate::changes::Changes;
use crate::server::{self, Identity};

/// `/d/<id>`: the document's folder, `/d/<id>/`, so that the links on its
/// pages resolve inside it.
pub async fn to_folder(UrlPath(id): UrlPath<String>) -> Response {
    axum::response::Redirect::permanent(&format!("/d/{id}/")).into_response()
}

/// `/d/<id>/`: the latest version's front page.
pub async fn latest_root(
    State(state): State<ServerMode>,
    UrlPath(id): UrlPath<String>,
    request: Request,
) -> Response {
    latest(&state, &id, "/", request).await
}

/// Anything under `/d/<id>/`: under `v/<n>/`, version `n` — mounted there —
/// and otherwise the latest version, mounted at `/d/<id>`, in place.
pub async fn in_document(
    State(state): State<ServerMode>,
    UrlPath((id, rest)): UrlPath<(String, String)>,
    request: Request,
) -> Response {
    if let Some((n, path)) = versioned(&rest) {
        return match path.as_str() {
            "/download" => super::pages::download(&state, &id, n).await,
            "/__devpage/versions" => super::pages::versions(&state, &id, Some(n)),
            _ => serve(&state, &id, n, &format!("/d/{id}/v/{n}"), &path, request).await,
        };
    }
    if rest == "__devpage/versions" {
        return super::pages::versions(&state, &id, None);
    }
    latest(&state, &id, &format!("/{rest}"), request).await
}

/// `v/<n>` or `v/<n>/<path>`: version `n`, and the path inside it.
fn versioned(rest: &str) -> Option<(u64, String)> {
    let after = rest.strip_prefix("v/")?;
    let (n, path) = after.split_once('/').unwrap_or((after, ""));
    Some((n.parse().ok()?, format!("/{path}")))
}

/// The request answered by the latest version, mounted at `/d/<id>`.
async fn latest(state: &ServerMode, id: &str, path: &str, request: Request) -> Response {
    match metrics::time(Stage::Store, || state.store.latest(id)) {
        Ok(Some(n)) => serve(state, id, n, &format!("/d/{id}"), path, request).await,
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

/// The request answered by version `n`'s router under `mount`, as if for
/// `path`.
async fn serve(
    state: &ServerMode,
    id: &str,
    n: u64,
    mount: &str,
    path: &str,
    request: Request,
) -> Response {
    let entry = match metrics::time(Stage::Store, || state.store.entry(id, n)) {
        Ok(Some(entry)) => entry,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
        }
    };
    let served = match version_router(state, id, n, mount, &entry).await {
        Ok(served) => served,
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
    // Held until the version has answered: it is not evicted meanwhile.
    served
        .router
        .clone()
        .oneshot(Request::from_parts(parts, Body::new(body)))
        .await
        .into_response()
}

/// Version `n`'s router under `mount`, held for one request: kept while the
/// version is in the cache, made on a miss — from the zip in the bucket when
/// the version isn't unpacked yet. `entry` is its document file, from the
/// top of the zip.
pub(super) async fn version_router(
    state: &ServerMode,
    id: &str,
    n: u64,
    mount: &str,
    entry: &str,
) -> Result<Served> {
    let key = (id.to_string(), n);
    let fetch = state.bucket.get_version(id, n);
    // The latest version's pages hear the document's publishes and its
    // comments, and show and take them; a version's own pages, behind their
    // banner, never change and are read as they were.
    let latest = mount == format!("/d/{id}");
    let changes = if latest {
        state.bus(id)
    } else {
        Changes::default()
    };
    state
        .cache
        .router(&key, mount, fetch, |dir: &Path| {
            let _made = Made(std::time::Instant::now());
            let served = ServedFolder::new(dir)?.mounted(mount);
            let document = Document::load(&served.path().join(entry), &served)?;
            // No search index until the version's first search asks for one.
            let index = Arc::new(Index::build(&served));
            let file = document.path();
            let mut options = server::Options::new(Arc::new(document), changes.clone());
            options.index = index.clone();
            let (searches, key) = (
                state.searches.clone(),
                (id.to_string(), n, mount.to_string()),
            );
            options.search_on_demand = Some(Arc::new(move || {
                searches.get(&key, || Search::frozen(&index, &file))
            }));
            options.comments = None;
            if latest {
                options.store = Arc::new(DocumentThreads {
                    store: state.store.clone(),
                    document: id.to_string(),
                    version: n,
                });
                options.comments = Some(match &state.user_header {
                    Some(header) => Identity::Header(header.clone()),
                    None => Identity::Local("anonymous".into()),
                });
            }
            // External addresses are the CLI's to ask, before it uploads: a
            // published page never waits on the internet.
            options.preview = Arc::new(Preview::new(
                crate::http::offline(),
                Arc::new(crate::terminal::Terminal),
            ));
            // The served folder carries the mount, so every address the
            // version's router makes — redirects included — is already under
            // it.
            Ok(server::router(options))
        })
        .await
}

/// Times a version's router as it is made, whichever way the making ends.
struct Made(std::time::Instant);

impl Drop for Made {
    fn drop(&mut self) {
        metrics::observe(Stage::Router, self.0.elapsed());
    }
}
