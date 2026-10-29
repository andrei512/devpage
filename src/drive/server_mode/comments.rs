//! A document's comments over the API — listed, started on the latest
//! version, replied to, resolved — and each open thread carried onto a new
//! version when it is published.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use axum::Json;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use devpage_core::changes::Change;
use devpage_core::comments::{self, Comments, anchor};

use super::ServerMode;
use super::api::{author, document_id};
use crate::adapters::sqlite::{DocumentThreads, Sqlite, ThreadRow};
use crate::drive::api_types::{NewThread, Refusal, Reply, Started, Thread};

/// `GET /api/documents/<id>/comments`: every thread, oldest first.
pub async fn list(
    State(state): State<ServerMode>,
    UrlPath(id): UrlPath<String>,
) -> Result<Response, Refusal> {
    let id = existing(&state, &id)?;
    let threads: Vec<Thread> = state
        .store
        .threads(&id, None)?
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
/// version, announced to its open pages.
pub async fn create(
    State(state): State<ServerMode>,
    UrlPath(id): UrlPath<String>,
    headers: HeaderMap,
    Json(new): Json<NewThread>,
) -> Result<Response, Refusal> {
    let author = author(&state, &headers)?;
    let id = document_id(&id)?;
    let version = state.store.latest(&id)?.ok_or(Refusal::NotFound)?;
    let threads = Comments::new(
        Arc::new(DocumentThreads {
            store: state.store.clone(),
            document: id.clone(),
            version,
        }),
        Arc::new(state.bus(&id)),
    );
    let thread = threads
        .create(&new.page, new.anchor, &author, &new.text)
        .map_err(|e| Refusal::Unprocessable(format!("{e:#}")))?;
    Ok((
        StatusCode::CREATED,
        Json(Started {
            id: thread,
            version,
        }),
    )
        .into_response())
}

/// `POST /api/documents/<id>/comments/<thread>/replies`.
pub async fn reply(
    State(state): State<ServerMode>,
    UrlPath((id, thread)): UrlPath<(String, String)>,
    headers: HeaderMap,
    Json(reply): Json<Reply>,
) -> Result<Response, Refusal> {
    let author = author(&state, &headers)?;
    let id = existing(&state, &id)?;
    let page = page_of(&state.store, &id, &thread)?;
    let (_, at) = comments::new_id();
    let message = comments::Message {
        author,
        at,
        text: reply.text,
    };
    if !state.store.reply(&id, None, &thread, &message)? {
        return Err(Refusal::NotFound);
    }
    state.bus(&id).publish(Change::Comments(page));
    Ok(StatusCode::CREATED.into_response())
}

/// `DELETE /api/documents/<id>/comments/<thread>`: resolved, so gone.
pub async fn resolve(
    State(state): State<ServerMode>,
    UrlPath((id, thread)): UrlPath<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, Refusal> {
    author(&state, &headers)?;
    let id = existing(&state, &id)?;
    let page = page_of(&state.store, &id, &thread)?;
    if !state.store.resolve(&id, None, &thread)? {
        return Err(Refusal::NotFound);
    }
    state.bus(&id).publish(Change::Comments(page));
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// The id of a document that exists.
fn existing(state: &ServerMode, id: &str) -> Result<String, Refusal> {
    let id = document_id(id)?;
    state.store.latest(&id)?.ok_or(Refusal::NotFound)?;
    Ok(id)
}

/// The page thread `thread` is on.
fn page_of(store: &Sqlite, id: &str, thread: &str) -> Result<String, Refusal> {
    if !comments::is_id(thread) {
        return Err(Refusal::NotFound);
    }
    store
        .threads(id, None)?
        .into_iter()
        .find(|row| row.id == thread)
        .map(|row| row.page)
        .ok_or(Refusal::NotFound)
}

/// Carries every open thread of the document onto version `version`, whose
/// files are unpacked in `files`: each finds its quote in its page's new
/// text, by its context where it can, and is detached when the quote — or
/// the page — is gone. The pages whose threads changed are announced.
pub async fn carry(state: &ServerMode, id: &str, version: u64, files: &Path) -> Result<()> {
    let store = state.store.clone();
    let (document, files) = (id.to_string(), files.to_path_buf());
    // Rendering a page draws its diagrams, which blocks.
    let changed =
        tokio::task::spawn_blocking(move || carry_onto(&store, &document, version, &files))
            .await??;
    let changes = state.bus(id);
    for page in changed {
        changes.publish(Change::Comments(page));
    }
    Ok(())
}

fn carry_onto(store: &Sqlite, id: &str, version: u64, files: &Path) -> Result<Vec<String>> {
    let mut pages: BTreeMap<String, Vec<ThreadRow>> = BTreeMap::new();
    for thread in store.threads(id, None)? {
        if !thread.detached {
            pages.entry(thread.page.clone()).or_default().push(thread);
        }
    }
    let mut changed = Vec::new();
    for (page, threads) in pages {
        let text = std::fs::read_to_string(files.join(page.trim_start_matches('/')))
            .ok()
            .map(|source| anchor::text(&devpage_core::render::markdown(&source)));
        let mut moved = false;
        for thread in threads {
            let found = text
                .as_deref()
                .and_then(|text| anchor::reanchor(&thread.anchor, text));
            if found.as_ref() != Some(&thread.anchor) {
                store.reanchor(id, &thread.id, version, found.as_ref())?;
                moved = true;
            }
        }
        if moved {
            changed.push(page);
        }
    }
    Ok(changed)
}
