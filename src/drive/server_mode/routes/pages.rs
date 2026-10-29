//! The server's own pages and what a published page asks of it beside its
//! document: the front page, a page's versions, and a version's download —
//! a PDF of a doc or slides, a PNG of a design.

use axum::http::{HeaderMap, header};
use axum::response::{Html, IntoResponse, Response};
use devpage_core::addresses::{Owned, address};
use devpage_core::page::escape;

use crate::drive::api_types::Refusal;
use crate::drive::server_mode::published::{Mount, Published};

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

/// `/favicon.ico`: devpage's logo, the tab icon of the front page and of
/// every published page whose document has no logo of its own.
pub async fn favicon() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "image/x-icon"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        FAVICON,
    )
}

/// `assets/logo.svg` drawn at 64 pixels, as an ICO: the one format every
/// browser takes at `/favicon.ico`.
const FAVICON: &[u8] = include_bytes!("../../../../assets/favicon.ico");

/// The front page, with `{{…}}` where the server fills it in.
const FRONT: &str = include_str!("../../../../assets/server-front.html");

/// devpage's logo: a copy of `docs/logo.svg`, which the image's build has
/// no `docs/` to read from.
const LOGO: &str = include_str!("../../../../assets/logo.svg");

/// `__devpage/versions` under a page's mount: the version the page shows —
/// version `n` under `/d/<id>/v/<n>`, the latest under `/d/<id>` — the
/// latest, every version with its link, and the page's download, for the
/// version menu, the banner on an old version and the download button.
pub fn versions(published: &Published, id: &str, mount: Mount) -> Result<Response, Refusal> {
    let (document, versions) = published.get(id)?;
    let shown = match mount {
        Mount::Latest => document.latest,
        Mount::Pinned(n) => n,
    };
    let version = versions
        .iter()
        .find(|v| v.number == shown)
        .ok_or(Refusal::NotFound)?;
    let download = devpage_core::document::downloads_as(&version.kind).map(|(extension, _)| {
        serde_json::json!({
            "url": format!("{}/download", Mount::Pinned(shown).path(id)),
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
                "url": format!("{}/", Mount::Pinned(v.number).path(id)),
            })
        })
        .collect();
    let body = serde_json::json!({
        "version": shown,
        "latest": document.latest,
        "pinned": matches!(mount, Mount::Pinned(_)),
        "latest_url": format!("{}/", Mount::Latest.path(id)),
        "download": download,
        "versions": listed,
    });
    Ok(([(header::CACHE_CONTROL, "no-cache")], axum::Json(body)).into_response())
}

/// `/d/<id>/v/<n>/download`: version `n` as a PDF or a PNG, rendered once
/// and then kept in the cache beside the version.
pub async fn download(published: &Published, id: &str, n: u64) -> Result<Response, Refusal> {
    let download = published.download(id, n).await?.ok_or(Refusal::NotFound)?;
    Ok((
        [
            (header::CONTENT_TYPE, download.media.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", download.name),
            ),
        ],
        download.bytes,
    )
        .into_response())
}
