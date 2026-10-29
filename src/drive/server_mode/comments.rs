//! A document's comments over the API — listed, started on the latest
//! version, replied to, resolved — and each open thread carried onto a new
//! version when it is published. Every write goes through the comments of a
//! version, which [`of_version`] makes for the API and for each version's
//! own pages alike.

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use axum::Json;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use devpage_core::comments::Comments;
use devpage_core::document::Document;
use devpage_core::served::ServedFolder;

use super::ServerMode;
use super::api::document_id;
use super::cache::Served;
use crate::adapters::sqlite::DocumentThreads;
use crate::drive::api_types::{NewThread, Refusal, Reply, Started, Thread};
use crate::server;

/// `GET /api/documents/<id>/comments`: every thread, oldest first.
pub async fn list(
    State(state): State<ServerMode>,
    UrlPath(id): UrlPath<String>,
) -> Result<Response, Refusal> {
    let id = existing(&state, &id)?;
    let threads: Vec<Thread> = state
        .store
        .threads(&id)?
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
    State(state): State<ServerMode>,
    UrlPath(id): UrlPath<String>,
    headers: HeaderMap,
    Json(new): Json<NewThread>,
) -> Result<Response, Refusal> {
    let author = server::author(&state.identity, &headers)?;
    let latest = Latest::of(&state, &document_id(&id)?).await?;
    let thread = latest
        .comments
        .create(&new.page, new.anchor, &author, &new.text)?;
    Ok((
        StatusCode::CREATED,
        Json(Started {
            id: thread,
            version: latest.version,
        }),
    )
        .into_response())
}

/// `POST /api/documents/<id>/comments/<thread>/replies`: by the thread's id
/// alone, detached or not.
pub async fn reply(
    State(state): State<ServerMode>,
    UrlPath((id, thread)): UrlPath<(String, String)>,
    headers: HeaderMap,
    Json(reply): Json<Reply>,
) -> Result<Response, Refusal> {
    let author = server::author(&state.identity, &headers)?;
    let latest = Latest::of(&state, &document_id(&id)?).await?;
    latest.comments.reply(&thread, &author, &reply.text)?;
    Ok(StatusCode::CREATED.into_response())
}

/// `DELETE /api/documents/<id>/comments/<thread>`: resolved, so gone.
pub async fn resolve(
    State(state): State<ServerMode>,
    UrlPath((id, thread)): UrlPath<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, Refusal> {
    server::author(&state.identity, &headers)?;
    let latest = Latest::of(&state, &document_id(&id)?).await?;
    latest.comments.resolve(&thread)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// The id of a document that exists.
fn existing(state: &ServerMode, id: &str) -> Result<String, Refusal> {
    let id = document_id(id)?;
    state.store.latest(&id)?.ok_or(Refusal::NotFound)?;
    Ok(id)
}

/// The comments of version `version` of document `id`, whose files are
/// `folder`: a thread started through them is on that version, a thread
/// carried through them is carried onto it, and every write is announced on
/// the document's bus.
pub(super) fn of_version(
    state: &ServerMode,
    id: &str,
    version: u64,
    folder: ServedFolder,
) -> Comments {
    Comments::new(
        Arc::new(DocumentThreads {
            store: state.store.clone(),
            document: id.to_string(),
            version,
        }),
        Arc::new(state.bus(id)),
        folder,
    )
}

/// A document's latest version, held from the cache while its comments are
/// written.
struct Latest {
    version: u64,
    comments: Comments,
    _held: Served,
}

impl Latest {
    async fn of(state: &ServerMode, id: &str) -> Result<Latest, Refusal> {
        let version = state.store.latest(id)?.ok_or(Refusal::NotFound)?;
        let entry = state.store.entry(id, version)?.ok_or(Refusal::NotFound)?;
        let mount = format!("/d/{id}");
        let held = super::versions::version_router(state, id, version, &mount, &entry).await?;
        let folder = ServedFolder::new(&held.folder)
            .map_err(anyhow::Error::from)?
            .mounted(&mount);
        Ok(Latest {
            version,
            comments: of_version(state, id, version, folder),
            _held: held,
        })
    }
}

/// Carries every open thread of the document onto version `version`, whose
/// files are unpacked in `files` with its document file at `entry`: each
/// finds its quote in its page as the page shows it, and is detached when
/// the quote — or the page — is gone. The pages whose threads changed are
/// announced.
pub async fn carry(
    state: &ServerMode,
    id: &str,
    version: u64,
    files: &Path,
    entry: &str,
) -> Result<()> {
    let folder = ServedFolder::new(files)?.mounted(&format!("/d/{id}"));
    let comments = of_version(state, id, version, folder.clone());
    let entry = folder.path().join(entry);
    // Rendering a page draws its diagrams, which blocks.
    tokio::task::spawn_blocking(move || comments.carry(&Document::load(&entry, &folder)?)).await?
}
