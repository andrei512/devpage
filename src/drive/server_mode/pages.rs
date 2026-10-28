//! The server's own pages and what a published page asks of it beside its
//! document: the front page, a page's versions, and a version's download —
//! a PDF of a doc or slides, a PNG of a design.

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use devpage_core::addresses::{Owned, address};
use devpage_core::designs::Designs;
use devpage_core::document::Document;
use devpage_core::render::escape;
use devpage_core::served::ServedFolder;
use devpage_design::Theme;

use super::ServerMode;
use super::versions::version_router;

/// `/`: that this is a devpage server, and how to connect to it. It lists no
/// documents: a document is reached only through its link.
pub async fn front(headers: HeaderMap) -> Html<String> {
    let host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("this-server");
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("http");
    let server = escape(&format!("{scheme}://{host}"));
    let logo = LOGO.trim();
    let favicon: String = logo
        .chars()
        .map(|c| match c {
            '#' => "%23".to_string(),
            '"' => "'".to_string(),
            '\n' => " ".to_string(),
            c => c.to_string(),
        })
        .collect();
    Html(
        FRONT
            .replace("{{stylesheet}}", &address(Owned::Asset("style.css")))
            .replace("{{favicon}}", &favicon)
            .replace("{{logo}}", logo)
            .replace("{{repository}}", env!("CARGO_PKG_REPOSITORY"))
            .replace("{{version}}", env!("CARGO_PKG_VERSION"))
            .replace("{{server}}", &server),
    )
}

/// The front page, with `{{…}}` where the server fills it in.
const FRONT: &str = include_str!("../../../assets/server-front.html");

/// devpage's logo: a copy of `docs/logo.svg`, which the image's build has
/// no `docs/` to read from.
const LOGO: &str = include_str!("../../../assets/logo.svg");

/// `__devpage/versions` under a page's mount: the version the page shows —
/// version `n` under `/d/<id>/v/<n>`, the latest under `/d/<id>` — the
/// latest, every version with its link, and the page's download, for the
/// version menu, the banner on an old version and the download button.
pub fn versions(state: &ServerMode, id: &str, pinned: Option<u64>) -> Response {
    let (document, versions) = match state.store.get(id) {
        Ok(Some(found)) => found,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
        }
    };
    let shown = pinned.unwrap_or(document.latest);
    let Some(version) = versions.iter().find(|v| v.number == shown) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let download = downloadable(&version.kind).map(|(extension, _)| {
        serde_json::json!({
            "url": format!("/d/{id}/v/{shown}/download"),
            "format": extension.to_uppercase(),
        })
    });
    let listed: Vec<serde_json::Value> = versions
        .iter()
        .rev()
        .map(|v| {
            serde_json::json!({
                "number": v.number,
                "published": v.published,
                "author": v.author,
                "url": format!("/d/{id}/v/{}/", v.number),
            })
        })
        .collect();
    let body = serde_json::json!({
        "version": shown,
        "latest": document.latest,
        "pinned": pinned.is_some(),
        "latest_url": format!("/d/{id}/"),
        "download": download,
        "versions": listed,
    });
    ([(header::CACHE_CONTROL, "no-cache")], axum::Json(body)).into_response()
}

/// What a version of `kind` downloads as: its extension and media type.
fn downloadable(kind: &str) -> Option<(&'static str, &'static str)> {
    match kind {
        "doc" | "slides" => Some(("pdf", "application/pdf")),
        "design" => Some(("png", "image/png")),
        _ => None,
    }
}

/// `/d/<id>/v/<n>/download`: version `n` as a PDF or a PNG, rendered once
/// and then kept in the cache beside the version.
pub async fn download(state: &ServerMode, id: &str, n: u64) -> Response {
    match rendered(state, id, n).await {
        Ok(Some((bytes, media, name))) => (
            [
                (header::CONTENT_TYPE, media.to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{name}\""),
                ),
            ],
            bytes,
        )
            .into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            eprintln!("download {id} v{n}: {error:#}");
            (StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}")).into_response()
        }
    }
}

/// Version `n`'s download, its media type and file name; `None` when there
/// is no such version or its kind has no download.
async fn rendered(
    state: &ServerMode,
    id: &str,
    n: u64,
) -> Result<Option<(Vec<u8>, &'static str, String)>> {
    let Some((_, versions)) = state.store.get(id)? else {
        return Ok(None);
    };
    let Some(version) = versions.into_iter().find(|v| v.number == n) else {
        return Ok(None);
    };
    let Some((extension, media)) = downloadable(&version.kind) else {
        return Ok(None);
    };
    let Some(entry) = state.store.entry(id, n)? else {
        return Ok(None);
    };
    // Held while it renders: the version is not evicted meanwhile.
    let served = version_router(state, id, n, &format!("/d/{id}/v/{n}"), &entry).await?;
    let scratch = state.scratch.clone();
    let bytes = state
        .cache
        .download(&served, extension, |folder| async move {
            render(&folder, &entry, extension, &scratch).await
        })
        .await?;
    let stem = crate::pdf::default_output(&version.title);
    let name = stem
        .with_extension(extension)
        .to_string_lossy()
        .into_owned();
    Ok(Some((bytes, media, name)))
}

/// The document at `entry` in `folder` rendered: printed by Chrome as
/// `devpage pdf` prints it, or a design painted as its PNG, in light.
async fn render(
    folder: &std::path::Path,
    entry: &str,
    extension: &str,
    scratch: &std::path::Path,
) -> Result<Vec<u8>> {
    let served = ServedFolder::new(folder)?;
    if extension == "png" {
        let designs = Designs::new(&served);
        let entry = entry.to_string();
        return tokio::task::spawn_blocking(move || designs.png(&entry, Theme::Light))
            .await?
            .map_err(anyhow::Error::msg);
    }
    let document = Document::load(&served.path().join(entry), &served)?;
    let output = scratch.join(format!("{}.pdf", uuid::Uuid::new_v4().simple()));
    let printed = crate::pdf::print(Arc::new(document), &output).await;
    let bytes = std::fs::read(&output).with_context(|| "reading the printed PDF".to_string());
    let _ = std::fs::remove_file(&output);
    printed?;
    let bytes = bytes?;
    if bytes.is_empty() {
        bail!("Chrome printed nothing");
    }
    Ok(bytes)
}
