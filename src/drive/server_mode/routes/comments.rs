//! A document's comments over the API — listed, started on the latest
//! version, replied to, resolved — each written through the comments of the
//! latest version, as its own pages write them.

use axum::Json;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

use super::{author, document_id};
use crate::drive::api_types::{NewThread, Refusal, Reply, Started, Thread};
use crate::drive::server_mode::published::{Mount, Published};

/// `GET /api/documents/<id>/comments`: every thread, oldest first.
pub async fn list(
    State(published): State<Published>,
    UrlPath(id): UrlPath<String>,
) -> Result<Response, Refusal> {
    let threads: Vec<Thread> = published
        .threads(&document_id(&id)?)?
        .into_iter()
        .map(|row| Thread {
            id: row.id,
            page: row.page,
            version: row.version,
            detached: row.detached,
            detached_in: row.detached_in,
            anchor: row.anchor,
            thread: row.thread,
        })
        .collect();
    Ok(Json(threads).into_response())
}

/// `POST /api/documents/<id>/comments`: a thread started on the latest
/// version, on a page that is a file in it, announced to its open pages.
pub async fn create(
    State(published): State<Published>,
    UrlPath(id): UrlPath<String>,
    headers: HeaderMap,
    Json(new): Json<NewThread>,
) -> Result<Response, Refusal> {
    let author = author(&published, &headers)?;
    let latest = published.version(&document_id(&id)?, Mount::Latest).await?;
    let thread = published
        .comments(&latest)?
        .create(&new.page, new.anchor, &author, &new.text)?;
    let started = Started {
        id: thread,
        version: latest.number,
    };
    Ok((StatusCode::CREATED, Json(started)).into_response())
}

/// `POST /api/documents/<id>/comments/<thread>/replies`: by the thread's id
/// alone, detached or not.
pub async fn reply(
    State(published): State<Published>,
    UrlPath((id, thread)): UrlPath<(String, String)>,
    headers: HeaderMap,
    Json(reply): Json<Reply>,
) -> Result<Response, Refusal> {
    let author = author(&published, &headers)?;
    let latest = published.version(&document_id(&id)?, Mount::Latest).await?;
    published
        .comments(&latest)?
        .reply(&thread, &author, &reply.text)?;
    Ok(StatusCode::CREATED.into_response())
}

/// `DELETE /api/documents/<id>/comments/<thread>`: resolved, so gone.
pub async fn resolve(
    State(published): State<Published>,
    UrlPath((id, thread)): UrlPath<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, Refusal> {
    author(&published, &headers)?;
    let latest = published.version(&document_id(&id)?, Mount::Latest).await?;
    published.comments(&latest)?.resolve(&thread)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
