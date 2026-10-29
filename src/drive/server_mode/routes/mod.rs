//! Server mode's HTTP: each request read into a call on the published
//! documents, and each answer — or refusal, through the one [`Refusal`] —
//! written back. Nothing here decides anything a publish, a deletion or a
//! lookup does.

mod api;
mod comments;
mod pages;
mod versions;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::HeaderMap;
use axum::routing::{any, delete, get, post};

use super::published::Published;
use crate::drive::api_types::Refusal;
use crate::drive::http;

/// Every route of `devpage server`, on `published`, taking uploads of up to
/// `max_upload_bytes`.
pub fn router(published: Published, max_upload_bytes: u64) -> Router {
    let api = Router::new()
        .route("/api/documents", post(api::create).get(api::list))
        .route("/api/documents/{id}", get(api::get_one).delete(api::delete))
        .route("/api/documents/{id}/versions", post(api::add_version))
        .route(
            "/api/documents/{id}/versions/{n}",
            delete(api::delete_version),
        )
        .route(
            "/api/documents/{id}/comments",
            get(comments::list).post(comments::create),
        )
        .route(
            "/api/documents/{id}/comments/{thread}",
            delete(comments::resolve),
        )
        .route(
            "/api/documents/{id}/comments/{thread}/replies",
            post(comments::reply),
        )
        .layer(DefaultBodyLimit::max(
            usize::try_from(max_upload_bytes).unwrap_or(usize::MAX),
        ));
    Router::new()
        .merge(api)
        .route("/", get(pages::front))
        .route("/favicon.ico", get(pages::favicon))
        .route("/d/{id}", get(versions::to_folder))
        .route("/d/{id}/", any(versions::latest_root))
        .route("/d/{id}/{*rest}", any(versions::in_document))
        .route("/__devpage/{asset}", get(http::asset))
        .with_state(published)
}

/// A document id as the server gives them: a UUID. Anything else names no
/// document — and never reaches a path on disk or in the bucket.
fn document_id(id: &str) -> Result<String, Refusal> {
    uuid::Uuid::parse_str(id)
        .map(|uuid| uuid.hyphenated().to_string())
        .map_err(|_| Refusal::NotFound)
}

/// The author of a write: whoever the request's header names, as the
/// server's identity reads it.
fn author(published: &Published, headers: &HeaderMap) -> Result<String, Refusal> {
    Ok(http::author(published.identity(), headers)?)
}
