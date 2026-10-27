//! `devpage server`: documents published one at a time as zips, kept in the
//! bucket, recorded in the store, and served from the cache on the PVC —
//! each version through the same router a local devpage uses, mounted at
//! `/d/<id>/v/<n>`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use anyhow::{Context, Result, bail};
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Path as UrlPath, Request, State};
use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use devpage_core::cache::{Entry, Eviction, Lru};
use devpage_core::document::{Document, Kind};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

use crate::adapters::bucket::Bucket;
use crate::adapters::sqlite::Sqlite;
use crate::changes::Changes;
use crate::server;

/// What `devpage server` reads from the environment.
#[derive(Debug, PartialEq)]
pub struct Config {
    pub data_dir: PathBuf,
    pub bucket_url: String,
    pub cache_max_bytes: u64,
}

impl Config {
    /// The configuration in `vars`, or the first variable that is missing or
    /// malformed — named, never its value.
    pub fn from_vars(vars: impl Fn(&str) -> Option<String>) -> Result<Config> {
        let required = |name: &str| vars(name).with_context(|| format!("{name} is not set"));
        let cache_max_bytes = match vars("DEVPAGE_CACHE_MAX_BYTES") {
            Some(text) => bytes(&text).context("DEVPAGE_CACHE_MAX_BYTES is not a size")?,
            // TODO: 80% of the volume under DEVPAGE_DATA_DIR.
            None => 1 << 30,
        };
        Ok(Config {
            data_dir: required("DEVPAGE_DATA_DIR")?.into(),
            bucket_url: required("DEVPAGE_BUCKET_URL")?,
            cache_max_bytes,
        })
    }
}

/// `8GB`, `512MB`, `100KB` or a number of bytes.
fn bytes(text: &str) -> Result<u64> {
    let text = text.trim();
    let (number, unit) = match text.find(|c: char| !c.is_ascii_digit()) {
        Some(at) => text.split_at(at),
        None => (text, ""),
    };
    let scale: u64 = match unit.trim().to_ascii_uppercase().as_str() {
        "" | "B" => 1,
        "KB" => 1 << 10,
        "MB" => 1 << 20,
        "GB" => 1 << 30,
        "TB" => 1 << 40,
        _ => bail!("unknown unit"),
    };
    Ok(number.parse::<u64>()? * scale)
}

/// Versions unpacked on the PVC, each with the router that serves it.
struct Cache {
    dir: PathBuf,
    limit: u64,
    policy: Box<dyn Eviction>,
    entries: Mutex<HashMap<(String, u64), Entry>>,
    routers: Mutex<HashMap<(String, u64), Router>>,
}

#[derive(Clone)]
struct ServerMode {
    store: Arc<Sqlite>,
    bucket: Bucket,
    cache: Arc<Cache>,
}

/// The `devpage server` router for `config`.
pub fn router(config: &Config) -> Result<Router> {
    std::fs::create_dir_all(config.data_dir.join("cache"))?;
    let state = ServerMode {
        store: Arc::new(Sqlite::open(&config.data_dir.join("store.db"))?),
        bucket: Bucket::open(&config.bucket_url)?,
        cache: Arc::new(Cache {
            dir: config.data_dir.join("cache").canonicalize()?,
            limit: config.cache_max_bytes,
            policy: Box::new(Lru),
            entries: Mutex::default(),
            routers: Mutex::default(),
        }),
    };
    Ok(Router::new()
        .route("/api/documents", post(create))
        .route("/api/documents/{id}/versions", post(add_version))
        .route("/d/{id}", get(latest))
        .route("/d/{id}/v/{n}", get(version_root))
        .route("/d/{id}/v/{n}/{*rest}", get(version))
        .route("/__devpage/{asset}", get(server::asset))
        .fallback(by_referer)
        .with_state(state))
}

/// `POST /api/documents`: a zip becomes version 1 of a new document.
async fn create(State(state): State<ServerMode>, zip: Bytes) -> Response {
    let id = uuid::Uuid::new_v4().to_string();
    answer(publish(&state, &id, None, zip.to_vec()).await)
}

/// `POST /api/documents/<id>/versions`: a zip becomes its next version.
async fn add_version(
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
fn unpack(zip: &[u8], into: &Path) -> Result<()> {
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

/// `/d/<id>`: the latest version's front page.
async fn latest(
    State(state): State<ServerMode>,
    UrlPath(id): UrlPath<String>,
    request: Request,
) -> Response {
    match state.store.latest(&id) {
        Ok(Some(n)) => serve(&state, &id, n, "/", request).await,
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn version_root(
    State(state): State<ServerMode>,
    UrlPath((id, n)): UrlPath<(String, u64)>,
    request: Request,
) -> Response {
    serve(&state, &id, n, "/", request).await
}

async fn version(
    State(state): State<ServerMode>,
    UrlPath((id, n, rest)): UrlPath<(String, u64, String)>,
    request: Request,
) -> Response {
    serve(&state, &id, n, &format!("/{rest}"), request).await
}

/// PROTOTYPE: a page's script and widgets ask absolute addresses —
/// `/__devpage/events`, widgets, comments, the design stream — so each is
/// sent to the version the asking page is.
async fn by_referer(State(state): State<ServerMode>, request: Request) -> Response {
    let from = request
        .headers()
        .get(axum::http::header::REFERER)
        .and_then(|r| r.to_str().ok())
        .and_then(|r| r.split_once("/d/").map(|(_, rest)| rest.to_string()));
    let Some(from) = from else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut parts = from.split('/');
    let (Some(id), Some("v"), Some(n)) = (parts.next(), parts.next(), parts.next()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(n) = n.parse::<u64>() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let (id, path) = (id.to_string(), request.uri().path().to_string());
    serve(&state, &id, n, &path, request).await
}

/// The request answered by version `n`'s router, as if for `path`.
async fn serve(state: &ServerMode, id: &str, n: u64, path: &str, request: Request) -> Response {
    match state.store.has(id, n) {
        Ok(true) => {}
        Ok(false) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
        }
    }
    let router = match version_router(state, id, n).await {
        Ok(router) => router,
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}")).into_response();
        }
    };
    let (mut parts, body) = request.into_parts();
    let query = parts
        .uri
        .query()
        .map(|q| format!("?{q}"))
        .unwrap_or_default();
    parts.uri = format!("{path}{query}")
        .parse::<Uri>()
        .unwrap_or_else(|_| Uri::from_static("/"));
    // A fresh request: the outer router's path parameters ride in the
    // extensions, and the version's router would read them as its own.
    parts.extensions = Default::default();
    router
        .oneshot(Request::from_parts(parts, Body::new(body)))
        .await
        .into_response()
}

/// Version `n`'s router: kept while it is in the cache, made on a miss from
/// the zip in the bucket.
async fn version_router(state: &ServerMode, id: &str, n: u64) -> Result<Router> {
    let key = (id.to_string(), n);
    let cache = &state.cache;
    if let Some(router) = cache.routers.lock().unwrap().get(&key).cloned() {
        if let Some(entry) = cache.entries.lock().unwrap().get_mut(&key) {
            entry.read = SystemTime::now();
        }
        return Ok(router);
    }
    let zip = state.bucket.get_version(id, n).await?;
    let dir = cache.dir.join(id).join(n.to_string());
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    unpack(&zip, &dir)?;
    let dir = dir.canonicalize()?;
    let mount = format!("/d/{id}/v/{n}");
    devpage_core::served::ServedFolder::mount_at(&dir, &mount);
    let document = Document::load(&dir, &dir)?;
    let mut options = server::Options::new(Arc::new(document), Changes::default());
    options.comments = None;
    // The served folder carries the mount, so every address the version's
    // router makes — redirects included — is already under it.
    let router = server::router(options);
    cache
        .routers
        .lock()
        .unwrap()
        .insert(key.clone(), router.clone());
    cache.entries.lock().unwrap().insert(
        key.clone(),
        Entry {
            document: id.to_string(),
            version: n,
            bytes: size_of(&dir),
            read: SystemTime::now(),
        },
    );
    evict(cache, &key);
    Ok(router)
}

/// Past its limit, the cache deletes what its policy chooses — never `serving`.
fn evict(cache: &Cache, serving: &(String, u64)) {
    let entries: Vec<Entry> = cache.entries.lock().unwrap().values().cloned().collect();
    for key in cache.policy.choose(&entries, cache.limit) {
        if &key == serving {
            continue;
        }
        cache.routers.lock().unwrap().remove(&key);
        cache.entries.lock().unwrap().remove(&key);
        let _ = std::fs::remove_dir_all(cache.dir.join(&key.0).join(key.1.to_string()));
    }
}

fn size_of(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => size_of(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
        let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, text) in files {
            out.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            out.write_all(text.as_bytes()).unwrap();
        }
        out.finish().unwrap().into_inner()
    }

    fn config(name: &str) -> Config {
        let dir =
            std::env::temp_dir().join(format!("devpage-server-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bucket")).unwrap();
        Config {
            data_dir: dir.join("data"),
            bucket_url: format!("file://{}", dir.join("bucket").display()),
            cache_max_bytes: 1 << 20,
        }
    }

    #[test]
    fn the_environment_is_read_and_a_missing_variable_named() {
        let vars = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| v.to_string())
            }
        };
        let config = Config::from_vars(vars(&[
            ("DEVPAGE_DATA_DIR", "/data"),
            ("DEVPAGE_BUCKET_URL", "s3://b/p"),
            ("DEVPAGE_CACHE_MAX_BYTES", "8GB"),
        ]))
        .unwrap();
        assert_eq!(config.cache_max_bytes, 8 << 30);
        let missing = Config::from_vars(vars(&[("DEVPAGE_DATA_DIR", "/data")])).unwrap_err();
        assert_eq!(missing.to_string(), "DEVPAGE_BUCKET_URL is not set");
    }

    #[tokio::test]
    async fn a_published_zip_is_kept_in_the_bucket_and_served_at_its_link() {
        let config = config("publish");
        let app = router(&config).unwrap();
        let zip = zip_of(&[
            (
                "devpage.yml",
                "kind: site\ntitle: Launch\nroot: README.md\n",
            ),
            ("README.md", "# Launch\n\nThe plan.\n"),
        ]);
        let response = app
            .clone()
            .oneshot(
                Request::post("/api/documents")
                    .body(Body::from(zip.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let answer: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let id = answer["id"].as_str().unwrap().to_string();
        assert_eq!(answer["url"], format!("/d/{id}"));

        // The bucket holds the zip byte for byte.
        let kept = config
            .bucket_url
            .strip_prefix("file://")
            .map(|dir| Path::new(dir).join(format!("documents/{id}/versions/1.zip")))
            .unwrap();
        assert_eq!(std::fs::read(kept).unwrap(), zip);

        // The cache is empty until a read; then the page renders from it.
        assert!(!config.data_dir.join("cache").join(&id).exists());
        let home = app
            .clone()
            .oneshot(
                Request::get(format!("/d/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // The front page leads to the root page, under the version's mount.
        assert_eq!(home.status(), StatusCode::TEMPORARY_REDIRECT);
        let location = home.headers()["location"].to_str().unwrap().to_string();
        assert!(location.starts_with(&format!("/d/{id}/v/1/")), "{location}");
        let page = app
            .clone()
            .oneshot(Request::get(&location).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = page.status();
        let html = axum::body::to_bytes(page.into_body(), usize::MAX)
            .await
            .unwrap();
        let html = String::from_utf8_lossy(&html);
        assert_eq!(status, StatusCode::OK, "{location}: {html}");
        assert!(html.contains("The plan."), "{html}");
        assert!(config.data_dir.join("cache").join(&id).join("1").exists());

        // A second publish is version 2, and the link now leads to it.
        let zip = zip_of(&[
            (
                "devpage.yml",
                "kind: site\ntitle: Launch\nroot: README.md\n",
            ),
            ("README.md", "# Launch\n\nThe new plan.\n"),
        ]);
        let response = app
            .clone()
            .oneshot(
                Request::post(format!("/api/documents/{id}/versions"))
                    .body(Body::from(zip))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let home = app
            .clone()
            .oneshot(
                Request::get(format!("/d/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let location = home.headers()["location"].to_str().unwrap().to_string();
        assert!(location.starts_with(&format!("/d/{id}/v/2/")), "{location}");
    }

    /// Past its limit the cache lets the least recently read version go, and
    /// a read of it again fetches it from the bucket.
    #[tokio::test]
    async fn past_its_limit_the_cache_evicts_and_a_read_fetches_again() {
        let mut config = config("evict");
        config.cache_max_bytes = 100;
        let app = router(&config).unwrap();
        let mut ids = Vec::new();
        for title in ["One", "Two"] {
            let zip = zip_of(&[
                (
                    "devpage.yml",
                    &format!("kind: site\ntitle: {title}\nroot: README.md\n"),
                ),
                (
                    "README.md",
                    &format!("# {title}\n\n{}\n", "words ".repeat(10)),
                ),
            ]);
            let response = app
                .clone()
                .oneshot(
                    Request::post("/api/documents")
                        .body(Body::from(zip))
                        .unwrap(),
                )
                .await
                .unwrap();
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let answer: serde_json::Value = serde_json::from_slice(&body).unwrap();
            ids.push(answer["id"].as_str().unwrap().to_string());
        }
        let read = |id: &str| {
            let app = app.clone();
            let id = id.to_string();
            async move {
                app.oneshot(
                    Request::get(format!("/d/{id}/v/1/README.md"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
                .status()
            }
        };
        let cached = |id: &str| config.data_dir.join("cache").join(id).join("1").exists();
        assert_eq!(read(&ids[0]).await, StatusCode::OK);
        assert!(cached(&ids[0]));
        assert_eq!(read(&ids[1]).await, StatusCode::OK);
        assert!(!cached(&ids[0]), "the older one went");
        assert!(cached(&ids[1]), "never the one being served");
        assert_eq!(read(&ids[0]).await, StatusCode::OK);
        assert!(cached(&ids[0]));
    }

    #[tokio::test]
    async fn a_broken_link_refuses_the_publish_with_its_problems() {
        let app = router(&config("broken")).unwrap();
        let zip = zip_of(&[
            ("devpage.yml", "kind: site\ntitle: T\nroot: README.md\n"),
            ("README.md", "# T\n\n[gone](gone.md)\n"),
        ]);
        let response = app
            .oneshot(
                Request::post("/api/documents")
                    .body(Body::from(zip))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&body).contains("gone.md"));
    }

    #[tokio::test]
    async fn a_zip_entry_outside_the_document_refuses_the_publish() {
        let app = router(&config("escape")).unwrap();
        let zip = zip_of(&[("../evil.md", "# x")]);
        let response = app
            .oneshot(
                Request::post("/api/documents")
                    .body(Body::from(zip))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}
