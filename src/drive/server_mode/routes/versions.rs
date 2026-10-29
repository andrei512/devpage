//! Serving a version: the request handed to that version's own router under
//! its mount — the latest at `/d/<id>`, every version at `/d/<id>/v/<n>`.

use axum::body::Body;
use axum::extract::{Path as UrlPath, Request, State};
use axum::http::Uri;
use axum::response::{IntoResponse, Redirect, Response};
use tower::ServiceExt;

use crate::drive::api_types::Refusal;
use crate::drive::server_mode::published::{Mount, Published};

/// `/d/<id>`: the document's folder, `/d/<id>/`, so that the links on its
/// pages resolve inside it.
pub async fn to_folder(UrlPath(id): UrlPath<String>) -> Response {
    Redirect::permanent(&format!("{}/", Mount::Latest.path(&id))).into_response()
}

/// `/d/<id>/`: the latest version's front page.
pub async fn latest_root(
    State(published): State<Published>,
    UrlPath(id): UrlPath<String>,
    request: Request,
) -> Result<Response, Refusal> {
    serve(&published, &id, Mount::Latest, "/", request).await
}

/// Anything under `/d/<id>/`: under `v/<n>/`, version `n` — mounted there —
/// and otherwise the latest version, mounted at `/d/<id>`, in place.
pub async fn in_document(
    State(published): State<Published>,
    UrlPath((id, rest)): UrlPath<(String, String)>,
    request: Request,
) -> Result<Response, Refusal> {
    let (mount, path) = match pinned(&rest) {
        Some((n, path)) => (Mount::Pinned(n), path),
        None => (Mount::Latest, format!("/{rest}")),
    };
    match (mount, path.as_str()) {
        (Mount::Pinned(n), "/download") => super::pages::download(&published, &id, n).await,
        (_, "/__devpage/versions") => super::pages::versions(&published, &id, mount),
        _ => serve(&published, &id, mount, &path, request).await,
    }
}

/// `v/<n>` or `v/<n>/<path>`: version `n`, and the path inside it.
fn pinned(rest: &str) -> Option<(u64, String)> {
    let after = rest.strip_prefix("v/")?;
    let (n, path) = after.split_once('/').unwrap_or((after, ""));
    Some((n.parse().ok()?, format!("/{path}")))
}

/// The request answered by the version `mount` shows, under that mount, as
/// if for `path`.
async fn serve(
    published: &Published,
    id: &str,
    mount: Mount,
    path: &str,
    request: Request,
) -> Result<Response, Refusal> {
    // Held until it has answered: it is not evicted meanwhile.
    let version = published.version(id, mount).await?;
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
    Ok(version
        .router()
        .oneshot(Request::from_parts(parts, Body::new(body)))
        .await
        .into_response())
}
