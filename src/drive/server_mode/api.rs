//! The API: publishing a document's first version and each one after,
//! listing documents, and deleting a document or one version.

use std::path::Path;

use anyhow::{Result, bail};
use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use devpage_core::document::{Document, Kind};
use sha2::{Digest, Sha256};

use super::ServerMode;
use crate::adapters::sqlite::{Deleted, Publish};

/// Why a request failed, as its answer.
pub enum Refused {
    NotFound,
    Unauthorized(String),
    Unprocessable(String),
    Failed(anyhow::Error),
}

impl IntoResponse for Refused {
    fn into_response(self) -> Response {
        match self {
            Refused::NotFound => StatusCode::NOT_FOUND.into_response(),
            Refused::Unauthorized(header) => (
                StatusCode::UNAUTHORIZED,
                format!("a write needs the {header} header"),
            )
                .into_response(),
            Refused::Unprocessable(problems) => {
                (StatusCode::UNPROCESSABLE_ENTITY, problems).into_response()
            }
            Refused::Failed(error) => {
                eprintln!("server error: {error:#}");
                (StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}")).into_response()
            }
        }
    }
}

impl From<anyhow::Error> for Refused {
    fn from(error: anyhow::Error) -> Refused {
        Refused::Failed(error)
    }
}

/// A document id as the server gives them: a UUID. Anything else names no
/// document — and never reaches a path on disk or in the bucket.
pub fn document_id(id: &str) -> Result<String, Refused> {
    uuid::Uuid::parse_str(id)
        .map(|uuid| uuid.hyphenated().to_string())
        .map_err(|_| Refused::NotFound)
}

/// Who is writing: the value of `DEVPAGE_USER_HEADER`, which a write must
/// carry when it is set; `anonymous` when it isn't.
fn author(state: &ServerMode, headers: &HeaderMap) -> Result<String, Refused> {
    let Some(header) = &state.user_header else {
        return Ok("anonymous".into());
    };
    headers
        .get(header.as_str())
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| Refused::Unauthorized(header.clone()))
}

/// A publish's query: `?entry=<path>`, the document file inside the zip —
/// `devpage.yml` when it is left out.
#[derive(serde::Deserialize)]
pub struct Published {
    entry: Option<String>,
}

impl Published {
    /// The entry, checked to be a path inside the zip.
    fn entry(&self) -> Result<String, Refused> {
        let entry = self.entry.as_deref().unwrap_or("devpage.yml");
        let inside = !entry.is_empty()
            && Path::new(entry)
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_)));
        if !inside {
            return Err(Refused::Unprocessable(format!(
                "entry {entry}: not a path inside the document"
            )));
        }
        Ok(entry.to_string())
    }
}

/// `POST /api/documents`: a zip becomes version 1 of a new document.
pub async fn create(
    State(state): State<ServerMode>,
    Query(query): Query<Published>,
    headers: HeaderMap,
    zip: Bytes,
) -> Result<Response, Refused> {
    let author = author(&state, &headers)?;
    let entry = query.entry()?;
    let id = uuid::Uuid::new_v4().hyphenated().to_string();
    let version = publish(&state, &id, &entry, &author, zip.to_vec()).await?;
    Ok(published(&id, version, true))
}

/// `POST /api/documents/<id>/versions`: a zip becomes its next version.
pub async fn add_version(
    State(state): State<ServerMode>,
    UrlPath(id): UrlPath<String>,
    Query(query): Query<Published>,
    headers: HeaderMap,
    zip: Bytes,
) -> Result<Response, Refused> {
    let author = author(&state, &headers)?;
    let entry = query.entry()?;
    let id = document_id(&id)?;
    if state.store.latest(&id)?.is_none() {
        return Err(Refused::NotFound);
    }
    let version = publish(&state, &id, &entry, &author, zip.to_vec()).await?;
    Ok(published(&id, version, false))
}

fn published(id: &str, version: u64, created: bool) -> Response {
    let url = format!("/d/{id}");
    let body = if created {
        serde_json::json!({ "id": id, "version": version, "url": url })
    } else {
        serde_json::json!({ "version": version, "url": url })
    };
    (StatusCode::CREATED, Json(body)).into_response()
}

/// Checks the zip, keeps it in the bucket, then records it as the
/// document's next version — a new document's first. One publish to a
/// document at a time, so each takes the number after the last.
async fn publish(
    state: &ServerMode,
    id: &str,
    entry: &str,
    author: &str,
    zip: Vec<u8>,
) -> Result<u64, Refused> {
    // Unpacked once to read and check it; the bucket keeps the zip unchanged.
    let scratch = state
        .scratch
        .join(uuid::Uuid::new_v4().simple().to_string());
    std::fs::create_dir_all(&scratch).map_err(anyhow::Error::from)?;
    let checked = check(&zip, entry, &scratch).await;
    let _ = std::fs::remove_dir_all(&scratch);
    let (kind, title) = checked.map_err(|e| Refused::Unprocessable(format!("{e:#}")))?;
    let sha256 = format!("{:x}", Sha256::digest(&zip));
    let size = zip.len() as u64;

    let lock = state.publishing(id);
    let _one_at_a_time = lock.lock().await;
    let version = state.store.next_number(id)?;
    state.bucket.put_version(id, version, zip).await?;
    state.store.record(
        id,
        version,
        &Publish {
            entry,
            kind,
            title: &title,
            author,
            size,
            sha256: &sha256,
        },
    )?;
    eprintln!("published {id} v{version} by {author}");
    state.announce(id);
    Ok(version)
}

/// What the zip is — its kind and title — or why it can't be published:
/// a file outside it, no document at `entry`, or a broken link inside it.
/// External addresses aren't asked: the CLI asks them before uploading, so a
/// publish never waits on the internet.
async fn check(zip: &[u8], entry: &str, scratch: &Path) -> Result<(&'static str, String)> {
    unpack(zip, scratch)?;
    let scratch = scratch.canonicalize()?;
    let file = scratch.join(entry);
    if !file.is_file() {
        bail!("{entry}: no such file in the zip");
    }
    let document = Document::load(&file, &scratch)?;
    let problems = devpage_core::check::document(&document, None).await;
    if !problems.is_empty() {
        let lines: Vec<String> = problems.iter().map(ToString::to_string).collect();
        bail!("{}", lines.join("\n"));
    }
    let kind = match document.kind {
        Kind::Site { .. } => "site",
        Kind::Doc { .. } => "doc",
        Kind::Slides { .. } => "slides",
        Kind::Design => "design",
    };
    Ok((kind, document.title))
}

/// Unpacks `zip` into `into`, refusing an entry that would land outside it.
pub fn unpack(zip: &[u8], into: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        if file.is_symlink() {
            bail!("{}: a symbolic link", file.name());
        }
        let Some(relative) = file.enclosed_name() else {
            bail!("{}: outside the document", file.name());
        };
        let target = into.join(relative);
        if file.is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&target)?;
        std::io::copy(&mut file, &mut out)?;
    }
    Ok(())
}

/// `GET /api/documents`: every document — id, kind, title, latest version.
pub async fn list(State(state): State<ServerMode>) -> Result<Response, Refused> {
    let documents: Vec<serde_json::Value> = state
        .store
        .list()?
        .into_iter()
        .map(|d| {
            serde_json::json!({
                "id": d.id, "kind": d.kind, "title": d.title, "latest": d.latest,
            })
        })
        .collect();
    Ok(Json(documents).into_response())
}

/// `GET /api/documents/<id>`: the document and its versions.
pub async fn get_one(
    State(state): State<ServerMode>,
    UrlPath(id): UrlPath<String>,
) -> Result<Response, Refused> {
    let id = document_id(&id)?;
    let (document, versions) = state.store.get(&id)?.ok_or(Refused::NotFound)?;
    Ok(Json(serde_json::json!({
        "id": document.id,
        "kind": document.kind,
        "title": document.title,
        "latest": document.latest,
        "created": document.created,
        "versions": versions,
    }))
    .into_response())
}

/// `DELETE /api/documents/<id>`: the document, its versions — in the store,
/// the bucket and the cache.
pub async fn delete(
    State(state): State<ServerMode>,
    UrlPath(id): UrlPath<String>,
    headers: HeaderMap,
) -> Result<Response, Refused> {
    let author = author(&state, &headers)?;
    let id = document_id(&id)?;
    let lock = state.publishing(&id);
    let _one_at_a_time = lock.lock().await;
    let numbers = state.store.delete(&id)?.ok_or(Refused::NotFound)?;
    for n in &numbers {
        state.bucket.delete_version(&id, *n).await?;
    }
    state.cache.remove_document(&id);
    eprintln!("deleted {id} by {author}");
    state.announce(&id);
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `DELETE /api/documents/<id>/versions/<n>`: one version. The one before
/// the latest becomes the latest; the only one takes the document with it.
pub async fn delete_version(
    State(state): State<ServerMode>,
    UrlPath((id, n)): UrlPath<(String, u64)>,
    headers: HeaderMap,
) -> Result<Response, Refused> {
    let author = author(&state, &headers)?;
    let id = document_id(&id)?;
    let lock = state.publishing(&id);
    let _one_at_a_time = lock.lock().await;
    let deleted = state.store.delete_version(&id, n)?;
    if deleted == Deleted::Nothing {
        return Err(Refused::NotFound);
    }
    state.bucket.delete_version(&id, n).await?;
    state.cache.remove(&(id.clone(), n));
    if deleted == Deleted::Document {
        state.cache.remove_document(&id);
    }
    state.announce(&id);
    eprintln!("deleted {id} v{n} by {author}");
    Ok(StatusCode::NO_CONTENT.into_response())
}
