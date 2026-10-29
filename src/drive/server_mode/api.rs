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
use devpage_core::served::ServedFolder;
use sha2::{Digest, Sha256};

use super::ServerMode;
use super::metrics::{self, Stage};
use crate::adapters::sqlite::{Deleted, DocumentRow, Publish, VersionRow};
use crate::drive::api_types::{Described, Listed, Published, Refusal, Version};

/// A document id as the server gives them: a UUID. Anything else names no
/// document — and never reaches a path on disk or in the bucket.
pub fn document_id(id: &str) -> Result<String, Refusal> {
    uuid::Uuid::parse_str(id)
        .map(|uuid| uuid.hyphenated().to_string())
        .map_err(|_| Refusal::NotFound)
}

/// Who is writing: the value of `DEVPAGE_USER_HEADER`, which a write must
/// carry when it is set; `anonymous` when it isn't.
pub(super) fn author(state: &ServerMode, headers: &HeaderMap) -> Result<String, Refusal> {
    let Some(header) = &state.user_header else {
        return Ok("anonymous".into());
    };
    headers
        .get(header.as_str())
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| Refusal::Unauthorized(format!("a write needs the {header} header")))
}

/// A publish's query: `?entry=<path>`, the document file inside the zip —
/// `devpage.yml` when it is left out.
#[derive(serde::Deserialize)]
pub struct Entry {
    entry: Option<String>,
}

impl Entry {
    /// The entry, checked to be a path inside the zip.
    fn entry(&self) -> Result<String, Refusal> {
        let entry = self.entry.as_deref().unwrap_or("devpage.yml");
        let inside = !entry.is_empty()
            && Path::new(entry)
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_)));
        if !inside {
            return Err(Refusal::Unprocessable(format!(
                "entry {entry}: not a path inside the document"
            )));
        }
        Ok(entry.to_string())
    }
}

/// `POST /api/documents`: a zip becomes version 1 of a new document.
pub async fn create(
    State(state): State<ServerMode>,
    Query(query): Query<Entry>,
    headers: HeaderMap,
    zip: Bytes,
) -> Result<Response, Refusal> {
    let author = author(&state, &headers)?;
    let entry = query.entry()?;
    let id = uuid::Uuid::new_v4().hyphenated().to_string();
    let version = publish(&state, &id, &entry, &author, zip.to_vec()).await?;
    Ok(published(id, version))
}

/// `POST /api/documents/<id>/versions`: a zip becomes its next version.
pub async fn add_version(
    State(state): State<ServerMode>,
    UrlPath(id): UrlPath<String>,
    Query(query): Query<Entry>,
    headers: HeaderMap,
    zip: Bytes,
) -> Result<Response, Refusal> {
    let author = author(&state, &headers)?;
    let entry = query.entry()?;
    let id = document_id(&id)?;
    if state.store.latest(&id)?.is_none() {
        return Err(Refusal::NotFound);
    }
    let version = publish(&state, &id, &entry, &author, zip.to_vec()).await?;
    Ok(published(id, version))
}

fn published(id: String, version: u64) -> Response {
    let url = format!("/d/{id}");
    (StatusCode::CREATED, Json(Published { id, version, url })).into_response()
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
) -> Result<u64, Refusal> {
    // Unpacked once to read and check it; the bucket keeps the zip unchanged.
    let scratch = state
        .scratch
        .join(uuid::Uuid::new_v4().simple().to_string());
    std::fs::create_dir_all(&scratch).map_err(anyhow::Error::from)?;
    // Kept until the publish is done: re-anchoring reads the new version's
    // pages from it.
    let _scratch = Scratch(scratch.clone());
    let (kind, title) = metrics::timed(Stage::PublishCheck, check(&zip, entry, &scratch))
        .await
        .map_err(|e| Refusal::Unprocessable(format!("{e:#}")))?;
    let sha256 = format!("{:x}", Sha256::digest(&zip));
    let size = zip.len() as u64;

    let lock = state.publishing(id);
    let _one_at_a_time = lock.lock().await;
    let version = state.store.next_number(id)?;
    metrics::timed(Stage::Upload, state.bucket.put_version(id, version, zip)).await?;
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
    super::comments::carry(state, id, version, &scratch).await?;
    state.announce(id);
    Ok(version)
}

/// A publish's scratch folder, removed however the publish ends.
struct Scratch(std::path::PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What the zip is — its kind and title — or why it can't be published:
/// a file outside it, no document at `entry`, or a broken link inside it.
/// External addresses aren't asked: the CLI asks them before uploading, so a
/// publish never waits on the internet.
async fn check(zip: &[u8], entry: &str, scratch: &Path) -> Result<(&'static str, String)> {
    unpack(zip, scratch)?;
    let scratch = ServedFolder::new(scratch)?;
    let file = scratch.path().join(entry);
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
pub async fn list(State(state): State<ServerMode>) -> Result<Response, Refusal> {
    let documents: Vec<Listed> = state.store.list()?.into_iter().map(listed).collect();
    Ok(Json(documents).into_response())
}

/// `GET /api/documents/<id>`: the document and its versions.
pub async fn get_one(
    State(state): State<ServerMode>,
    UrlPath(id): UrlPath<String>,
) -> Result<Response, Refusal> {
    let id = document_id(&id)?;
    let (document, versions) = state.store.get(&id)?.ok_or(Refusal::NotFound)?;
    Ok(Json(described(document, versions)).into_response())
}

fn listed(row: DocumentRow) -> Listed {
    Listed {
        id: row.id,
        kind: row.kind,
        title: row.title,
        latest: row.latest,
    }
}

fn described(row: DocumentRow, versions: Vec<VersionRow>) -> Described {
    Described {
        id: row.id,
        kind: row.kind,
        title: row.title,
        latest: row.latest,
        created: row.created,
        versions: versions
            .into_iter()
            .map(|v| Version {
                number: v.number,
                kind: v.kind,
                title: v.title,
                author: v.author,
                published: v.published,
                size: v.size,
                sha256: v.sha256,
            })
            .collect(),
    }
}

/// `DELETE /api/documents/<id>`: the document, its versions — in the store,
/// the bucket and the cache.
pub async fn delete(
    State(state): State<ServerMode>,
    UrlPath(id): UrlPath<String>,
    headers: HeaderMap,
) -> Result<Response, Refusal> {
    let author = author(&state, &headers)?;
    let id = document_id(&id)?;
    let lock = state.publishing(&id);
    let _one_at_a_time = lock.lock().await;
    let numbers = state.store.delete(&id)?.ok_or(Refusal::NotFound)?;
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
) -> Result<Response, Refusal> {
    let author = author(&state, &headers)?;
    let id = document_id(&id)?;
    let lock = state.publishing(&id);
    let _one_at_a_time = lock.lock().await;
    let deleted = state.store.delete_version(&id, n)?;
    if deleted == Deleted::Nothing {
        return Err(Refusal::NotFound);
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
