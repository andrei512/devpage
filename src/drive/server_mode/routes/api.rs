//! The API: publishing a document's first version and each one after,
//! listing documents, and deleting a document or one version.

use std::path::Path;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

use super::{author, document_id};
use crate::adapters::sqlite::{DocumentRow, VersionRow};
use crate::drive::api_types::{Described, Listed, Published as Answer, Refusal, Version};
use crate::drive::server_mode::published::{Deletion, Mount, Published};

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
    State(published): State<Published>,
    Query(query): Query<Entry>,
    headers: HeaderMap,
    zip: Bytes,
) -> Result<Response, Refusal> {
    let author = author(&published, &headers)?;
    let entry = query.entry()?;
    let (id, version) = published
        .publish(None, &entry, &author, zip.to_vec())
        .await?;
    Ok(answer(id, version))
}

/// `POST /api/documents/<id>/versions`: a zip becomes its next version.
pub async fn add_version(
    State(published): State<Published>,
    UrlPath(id): UrlPath<String>,
    Query(query): Query<Entry>,
    headers: HeaderMap,
    zip: Bytes,
) -> Result<Response, Refusal> {
    let author = author(&published, &headers)?;
    let entry = query.entry()?;
    let (id, version) = published
        .publish(Some(&document_id(&id)?), &entry, &author, zip.to_vec())
        .await?;
    Ok(answer(id, version))
}

fn answer(id: String, version: u64) -> Response {
    let url = Mount::Latest.path(&id);
    (StatusCode::CREATED, Json(Answer { id, version, url })).into_response()
}

/// `GET /api/documents`: every document — id, kind, title, latest version.
pub async fn list(State(published): State<Published>) -> Result<Response, Refusal> {
    let documents: Vec<Listed> = published.list()?.into_iter().map(listed).collect();
    Ok(Json(documents).into_response())
}

/// `GET /api/documents/<id>`: the document and its versions.
pub async fn get_one(
    State(published): State<Published>,
    UrlPath(id): UrlPath<String>,
) -> Result<Response, Refusal> {
    let (document, versions) = published.get(&document_id(&id)?)?;
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
    State(published): State<Published>,
    UrlPath(id): UrlPath<String>,
    headers: HeaderMap,
) -> Result<Response, Refusal> {
    let author = author(&published, &headers)?;
    published
        .delete(&document_id(&id)?, Deletion::Document, &author)
        .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `DELETE /api/documents/<id>/versions/<n>`: one version. The one before
/// the latest becomes the latest; the only one takes the document with it.
pub async fn delete_version(
    State(published): State<Published>,
    UrlPath((id, n)): UrlPath<(String, u64)>,
    headers: HeaderMap,
) -> Result<Response, Refusal> {
    let author = author(&published, &headers)?;
    published
        .delete(&document_id(&id)?, Deletion::Version(n), &author)
        .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
