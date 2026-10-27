//! The API: publishing a document's first version and each one after.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use axum::body::Bytes;
use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use devpage_core::document::{Document, Kind};
use sha2::{Digest, Sha256};

use super::ServerMode;

/// `POST /api/documents`: a zip becomes version 1 of a new document.
pub async fn create(State(state): State<ServerMode>, zip: Bytes) -> Response {
    let id = uuid::Uuid::new_v4().to_string();
    answer(publish(&state, &id, None, zip.to_vec()).await)
}

/// `POST /api/documents/<id>/versions`: a zip becomes its next version.
pub async fn add_version(
    State(state): State<ServerMode>,
    UrlPath(id): UrlPath<String>,
    zip: Bytes,
) -> Response {
    let latest = match state.store.latest(&id) {
        Ok(Some(n)) => n,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
        }
    };
    answer(publish(&state, &id, Some(latest), zip.to_vec()).await)
}

fn answer(published: Result<(String, u64)>) -> Response {
    match published {
        Ok((id, version)) => (
            StatusCode::CREATED,
            axum::Json(
                serde_json::json!({ "id": id, "version": version, "url": format!("/d/{id}") }),
            ),
        )
            .into_response(),
        Err(error) => (StatusCode::UNPROCESSABLE_ENTITY, format!("{error:#}")).into_response(),
    }
}

/// Checks the zip, keeps it in the bucket, then records it: version 1 of a
/// new document, or the one after `latest`.
async fn publish(
    state: &ServerMode,
    id: &str,
    latest: Option<u64>,
    zip: Vec<u8>,
) -> Result<(String, u64)> {
    // Unpacked once to read and check it; the bucket keeps the zip unchanged.
    let scratch = tempdir(&state.cache.dir, id)?;
    let checked = check(&zip, &scratch).await;
    let _ = std::fs::remove_dir_all(&scratch);
    let (kind, title) = checked?;
    let sha256 = format!("{:x}", Sha256::digest(&zip));
    let size = zip.len() as u64;
    let version = latest.map_or(1, |n| n + 1);
    state.bucket.put_version(id, version, zip).await?;
    match latest {
        None => state.store.create(id, kind, &title, size, &sha256)?,
        Some(_) => state
            .store
            .add_version(id, version, &title, size, &sha256)?,
    }
    Ok((id.to_string(), version))
}

/// What the zip is — its kind and title — or why it can't be published:
/// an entry outside it, no document, or a broken link inside it.
async fn check(zip: &[u8], scratch: &Path) -> Result<(&'static str, String)> {
    unpack(zip, scratch)?;
    let document = Document::load(scratch, scratch)?;
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

fn tempdir(cache: &Path, id: &str) -> Result<PathBuf> {
    let dir = cache.join(format!(".publish-{id}"));
    std::fs::create_dir_all(&dir)?;
    Ok(dir.canonicalize()?)
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
