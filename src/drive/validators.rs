//! Validators: every response a page can ask for again carries an ETag, and a
//! request that names the ETag it has is answered `304 Not Modified`.
//!
//! A response that sets its own ETag — a built-in asset (its content hash), a
//! design's PNG (a hash of its scene) — keeps it; any other `200` to a `GET`
//! gets a strong ETag from the hash of its body. A response that sets no
//! `Cache-Control` gets `no-cache`: the browser may keep it, but asks first.

use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// The middleware: an ETag on every `200` to a `GET`, and `304` when the
/// request already has it. Streams (the event stream, the watch stream) are
/// routed around it — it reads the whole body.
pub async fn validate(request: Request, next: Next) -> Response {
    let get = matches!(*request.method(), Method::GET | Method::HEAD);
    let asked = request.headers().get(header::IF_NONE_MATCH).cloned();
    let response = next.run(request).await;
    if !get || response.status() != StatusCode::OK {
        return response;
    }
    let (mut parts, body) = response.into_parts();
    parts
        .headers
        .entry(header::CACHE_CONTROL)
        .or_insert(HeaderValue::from_static("no-cache"));
    let body = if parts.headers.contains_key(header::ETAG) {
        body
    } else {
        let bytes = axum::body::to_bytes(body, usize::MAX)
            .await
            .unwrap_or_default();
        if let Ok(value) = HeaderValue::from_str(&strong(&devpage_core::addresses::hash(&bytes))) {
            parts.headers.insert(header::ETAG, value);
        }
        Body::from(bytes)
    };
    let tag = parts.headers.get(header::ETAG).cloned();
    if let (Some(asked), Some(tag)) = (asked, tag)
        && matches(&asked, &tag)
    {
        return not_modified(parts.headers);
    }
    Response::from_parts(parts, body)
}

/// `"<hash>"`: a strong ETag.
pub fn strong(hash: &str) -> String {
    format!("\"{hash}\"")
}

/// Whether an `If-None-Match` names `tag`. The comparison is weak, as the
/// header's is: a proxy that weakens an ETag (`W/"…"`) while compressing still
/// gets its `304`.
pub fn matches(if_none_match: &HeaderValue, tag: &HeaderValue) -> bool {
    let bare = |value: &str| value.trim().trim_start_matches("W/").to_string();
    let (Ok(asked), Ok(tag)) = (if_none_match.to_str(), tag.to_str()) else {
        return false;
    };
    let tag = bare(tag);
    asked
        .split(',')
        .any(|candidate| candidate.trim() == "*" || bare(candidate) == tag)
}

/// Whether `headers` ask again for what `tag` names — for a response that
/// can skip its own work, as a design's PNG skips painting.
pub fn already_has(headers: &HeaderMap, tag: &str) -> bool {
    match (
        headers.get(header::IF_NONE_MATCH),
        HeaderValue::from_str(tag).ok(),
    ) {
        (Some(asked), Some(tag)) => matches(asked, &tag),
        _ => false,
    }
}

/// `304 Not Modified`, keeping the validator and caching headers the full
/// response would have carried.
pub fn not_modified(headers: HeaderMap) -> Response {
    let mut response = StatusCode::NOT_MODIFIED.into_response();
    for name in [header::ETAG, header::CACHE_CONTROL, header::VARY] {
        if let Some(value) = headers.get(&name) {
            response.headers_mut().insert(name, value.clone());
        }
    }
    response
}

/// The encoding to send, of those available: brotli, then gzip, as the
/// request's `Accept-Encoding` allows; `None` for the bytes as they are.
pub fn encoding(headers: &HeaderMap, brotli: bool, gzip: bool) -> Option<&'static str> {
    let accepted = headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let accepts = |wanted: &str| {
        accepted.split(',').any(|item| {
            let mut parts = item.split(';');
            let name = parts.next().unwrap_or("").trim();
            let refused = parts.any(|param| {
                param
                    .trim()
                    .strip_prefix("q=")
                    .and_then(|q| q.trim().parse::<f32>().ok())
                    == Some(0.0)
            });
            name.eq_ignore_ascii_case(wanted) && !refused
        })
    };
    if brotli && accepts("br") {
        Some("br")
    } else if gzip && accepts("gzip") {
        Some("gzip")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str) -> HeaderValue {
        HeaderValue::from_str(text).unwrap()
    }

    #[test]
    fn if_none_match_compares_weakly_and_takes_lists() {
        let tag = value("\"abc\"");
        assert!(matches(&value("\"abc\""), &tag));
        assert!(matches(&value("W/\"abc\""), &tag));
        assert!(matches(&value("\"x\", W/\"abc\""), &tag));
        assert!(matches(&value("*"), &tag));
        assert!(!matches(&value("\"abd\""), &tag));
    }

    #[test]
    fn brotli_is_preferred_and_a_zero_q_refuses() {
        let with = |accept: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(header::ACCEPT_ENCODING, value(accept));
            headers
        };
        assert_eq!(
            encoding(&with("gzip, deflate, br, zstd"), true, true),
            Some("br")
        );
        assert_eq!(encoding(&with("gzip"), true, true), Some("gzip"));
        assert_eq!(encoding(&with("br;q=0, gzip"), true, true), Some("gzip"));
        assert_eq!(encoding(&with("br"), false, true), None);
        assert_eq!(encoding(&HeaderMap::new(), true, true), None);
    }
}
