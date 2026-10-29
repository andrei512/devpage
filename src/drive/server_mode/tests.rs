use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::body::Body;
use axum::extract::Request;
use axum::http::StatusCode;
use devpage_core::cache::Lru;
use sha2::{Digest, Sha256};
use tower::ServiceExt;

use super::cache::Cache;
use super::{Config, router};

pub(super) fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, text) in files {
        out.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        out.write_all(text.as_bytes()).unwrap();
    }
    out.finish().unwrap().into_inner()
}

pub(super) fn config(name: &str) -> Config {
    let dir = std::env::temp_dir().join(format!("devpage-server-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Config::local(&dir.join("data"), &dir.join("bucket"), 1 << 20)
}

fn bucket_dir(config: &Config) -> PathBuf {
    PathBuf::from(config.bucket_url.strip_prefix("file://").unwrap())
}

pub(super) fn site(title: &str, text: &str) -> Vec<u8> {
    zip_of(&[
        (
            "devpage.yml",
            &format!("kind: site\ntitle: {title}\nroot: README.md\n"),
        ),
        ("README.md", &format!("# {title}\n\n{text}\n")),
    ])
}

/// What `app` answers to `request`: its status and body as JSON, or as text
/// in a JSON string when it isn't JSON.
pub(super) async fn ask(app: &Router, request: Request) -> (StatusCode, serde_json::Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value = serde_json::from_slice(&body)
        .unwrap_or_else(|_| serde_json::Value::String(String::from_utf8_lossy(&body).into()));
    (status, value)
}

pub(super) fn post(path: &str, zip: Vec<u8>) -> Request {
    Request::post(path).body(Body::from(zip)).unwrap()
}

pub(super) fn get(path: &str) -> Request {
    Request::get(path).body(Body::empty()).unwrap()
}

pub(super) fn delete(path: &str) -> Request {
    Request::delete(path).body(Body::empty()).unwrap()
}

/// Publishes `zip` as a new document: its id.
pub(super) async fn create(app: &Router, zip: Vec<u8>) -> String {
    let (status, answer) = ask(app, post("/api/documents", zip)).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    answer["id"].as_str().unwrap().to_string()
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
    let get = |path: String| {
        let app = app.clone();
        async move {
            let response = app
                .oneshot(Request::get(&path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            let status = response.status();
            let location = response
                .headers()
                .get("location")
                .map(|l| l.to_str().unwrap().to_string());
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            (
                status,
                location,
                String::from_utf8_lossy(&body).into_owned(),
            )
        }
    };
    // The link is the document's folder, and its front page leads to
    // the root page — the latest version, in place, under `/d/<id>`.
    let (status, location, _) = get(format!("/d/{id}")).await;
    assert_eq!(status, StatusCode::PERMANENT_REDIRECT);
    assert_eq!(location.unwrap(), format!("/d/{id}/"));
    let (status, location, _) = get(format!("/d/{id}/")).await;
    assert_eq!(status, StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(location.unwrap(), format!("/d/{id}/README.md"));
    let (status, _, html) = get(format!("/d/{id}/README.md")).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains("The plan."), "{html}");
    assert!(html.contains(&format!(r#"data-mount="/d/{id}""#)), "{html}");
    assert!(config.data_dir.join("cache").join(&id).join("1").exists());

    // A second publish is version 2: the link shows it, and version 1
    // stays at its own address, under its own mount.
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
    let (_, _, html) = get(format!("/d/{id}/README.md")).await;
    assert!(html.contains("The new plan."), "{html}");
    let (status, _, html) = get(format!("/d/{id}/v/1/README.md")).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains("The plan."), "{html}");
    assert!(
        html.contains(&format!(r#"data-mount="/d/{id}/v/1""#)),
        "{html}"
    );
    let (status, location, _) = get(format!("/d/{id}/v/1/")).await;
    assert_eq!(status, StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(location.unwrap(), format!("/d/{id}/v/1/README.md"));
    let (status, _, _) = get(format!("/d/{id}/v/3/README.md")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Past its limit the cache lets the least recently read version go, and
/// a read of it again fetches it from the bucket.
#[tokio::test]
async fn past_its_limit_the_cache_evicts_and_a_read_fetches_again() {
    let mut config = config("evict");
    // Room for one version, not two.
    config.cache_max_bytes = 150;
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

#[tokio::test]
async fn an_upload_over_the_limit_is_refused_with_413() {
    let mut config = config("too-big");
    config.max_upload_bytes = 1000;
    let app = router(&config).unwrap();
    let zip = site_with_noise(5000);
    let (status, _) = ask(&app, post("/api/documents", zip)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    // Bigger than axum's own 2 MB default, under the limit: accepted.
    config.max_upload_bytes = 10 << 20;
    let app = router(&config).unwrap();
    let zip = site_with_noise(3 << 20);
    assert!(zip.len() > 2 << 20);
    let (status, answer) = ask(&app, post("/api/documents", zip)).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
}

/// A site holding `bytes` of noise, stored uncompressed: a zip at least that
/// big.
fn site_with_noise(bytes: usize) -> Vec<u8> {
    let mut noise = Vec::with_capacity(bytes);
    let mut seed = 1u64;
    for _ in 0..bytes {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        noise.push((seed >> 33) as u8);
    }
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    out.start_file("devpage.yml", stored).unwrap();
    out.write_all(b"kind: site\ntitle: Big\nroot: README.md\n")
        .unwrap();
    out.start_file("README.md", stored).unwrap();
    out.write_all(b"# Big\n").unwrap();
    out.start_file("noise.bin", stored).unwrap();
    out.write_all(&noise).unwrap();
    let zip = out.finish().unwrap().into_inner();
    assert!(zip.len() > bytes);
    zip
}

#[tokio::test]
async fn with_a_user_header_a_write_needs_it_and_records_its_author() {
    let mut config = config("authors");
    config.user_header = Some("X-Forwarded-Email".into());
    let app = router(&config).unwrap();
    let (status, _) = ask(&app, post("/api/documents", site("A", "a"))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let signed = |path: &str, zip: Vec<u8>| {
        Request::post(path)
            .header("X-Forwarded-Email", "ana@example.com")
            .body(Body::from(zip))
            .unwrap()
    };
    let (status, answer) = ask(&app, signed("/api/documents", site("A", "a"))).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = answer["id"].as_str().unwrap().to_string();
    let path = format!("/api/documents/{id}/versions");
    let (status, _) = ask(&app, post(&path, site("A", "b"))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = ask(&app, delete(&format!("/api/documents/{id}"))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // Reads need nothing.
    let (status, document) = ask(&app, get(&format!("/api/documents/{id}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(document["versions"][0]["author"], "ana@example.com");

    // Without the setting, every write is anonymous.
    let app = router(&self::config("anonymous")).unwrap();
    let id = create(&app, site("A", "a")).await;
    let (_, document) = ask(&app, get(&format!("/api/documents/{id}"))).await;
    assert_eq!(document["versions"][0]["author"], "anonymous");
}

#[tokio::test]
async fn documents_are_listed_and_read_with_their_versions() {
    let app = router(&config("list")).unwrap();
    let (_, listed) = ask(&app, get("/api/documents")).await;
    assert_eq!(listed, serde_json::json!([]));
    let launch = create(&app, site("Launch", "one")).await;
    let path = format!("/api/documents/{launch}/versions");
    let (status, answer) = ask(&app, post(&path, site("Launch, again", "two"))).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(answer["version"], 2);
    assert_eq!(answer["url"], format!("/d/{launch}"));
    let other = create(&app, site("Other", "x")).await;

    let (status, listed) = ask(&app, get("/api/documents")).await;
    assert_eq!(status, StatusCode::OK);
    let listed = listed.as_array().unwrap();
    assert_eq!(listed.len(), 2);
    let first = listed.iter().find(|d| d["id"] == launch).unwrap();
    assert_eq!(first["kind"], "site");
    assert_eq!(first["title"], "Launch, again");
    assert_eq!(first["latest"], 2);
    assert!(listed.iter().any(|d| d["id"] == other));

    let (status, document) = ask(&app, get(&format!("/api/documents/{launch}"))).await;
    assert_eq!(status, StatusCode::OK);
    let versions = document["versions"].as_array().unwrap();
    let numbers: Vec<_> = versions.iter().map(|v| v["number"].clone()).collect();
    assert_eq!(numbers, [1, 2]);
    assert_eq!(versions[0]["title"], "Launch");

    // An id that is no document, or no id at all.
    let missing = uuid::Uuid::new_v4();
    for path in [
        format!("/api/documents/{missing}"),
        "/api/documents/..%2F..%2Fetc".to_string(),
        "/api/documents/not-an-id".to_string(),
    ] {
        assert_eq!(
            ask(&app, get(&path)).await.0,
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
    let (status, _) = ask(
        &app,
        post(
            &format!("/api/documents/{missing}/versions"),
            site("X", "x"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn deleting_a_document_takes_it_from_the_store_the_bucket_and_the_cache() {
    let config = config("delete");
    let app = router(&config).unwrap();
    let id = create(&app, site("Gone", "one")).await;
    let path = format!("/api/documents/{id}/versions");
    ask(&app, post(&path, site("Gone", "two"))).await;
    assert_eq!(
        ask(&app, get(&format!("/d/{id}/v/1/README.md"))).await.0,
        StatusCode::OK
    );
    assert!(config.data_dir.join("cache").join(&id).exists());

    let (status, _) = ask(&app, delete(&format!("/api/documents/{id}"))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(
        !bucket_dir(&config)
            .join("documents")
            .join(&id)
            .join("versions/1.zip")
            .exists()
    );
    assert!(
        !bucket_dir(&config)
            .join("documents")
            .join(&id)
            .join("versions/2.zip")
            .exists()
    );
    assert!(!config.data_dir.join("cache").join(&id).exists());
    assert_eq!(
        ask(&app, get(&format!("/api/documents/{id}"))).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        ask(&app, get(&format!("/d/{id}/v/1/README.md"))).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        ask(&app, delete(&format!("/api/documents/{id}"))).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn deleting_the_latest_version_makes_the_one_before_it_latest() {
    let config = config("delete-version");
    let app = router(&config).unwrap();
    let id = create(&app, site("Doc", "one")).await;
    let path = format!("/api/documents/{id}/versions");
    ask(&app, post(&path, site("Doc", "two"))).await;
    ask(&app, get(&format!("/d/{id}/v/2/README.md"))).await;

    let (status, _) = ask(&app, delete(&format!("/api/documents/{id}/versions/2"))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(
        !bucket_dir(&config)
            .join(format!("documents/{id}/versions/2.zip"))
            .exists()
    );
    assert!(!config.data_dir.join("cache").join(&id).join("2").exists());
    let (_, document) = ask(&app, get(&format!("/api/documents/{id}"))).await;
    assert_eq!(document["latest"], 1);
    assert_eq!(
        ask(&app, get(&format!("/d/{id}/v/2/README.md"))).await.0,
        StatusCode::NOT_FOUND
    );
    // The next publish never takes the deleted number.
    let (_, answer) = ask(&app, post(&path, site("Doc", "three"))).await;
    assert_eq!(answer["version"], 3);
    assert_eq!(
        ask(&app, delete(&format!("/api/documents/{id}/versions/9")))
            .await
            .0,
        StatusCode::NOT_FOUND
    );

    // Deleting the only one deletes the document.
    let only = create(&app, site("Only", "one")).await;
    let (status, _) = ask(&app, delete(&format!("/api/documents/{only}/versions/1"))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        ask(&app, get(&format!("/api/documents/{only}"))).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_publishes_to_one_document_take_one_number_each() {
    let config = config("concurrent");
    let app = router(&config).unwrap();
    let id = create(&app, site("Busy", "zero")).await;
    let path = format!("/api/documents/{id}/versions");
    let publishes = (0..8).map(|i| {
        let (app, path) = (app.clone(), path.clone());
        tokio::spawn(async move { ask(&app, post(&path, site("Busy", &format!("v{i}")))).await })
    });
    let mut numbers = Vec::new();
    for publish in publishes {
        let (status, answer) = publish.await.unwrap();
        assert_eq!(status, StatusCode::CREATED, "{answer}");
        numbers.push(answer["version"].as_u64().unwrap());
    }
    numbers.sort();
    assert_eq!(numbers, (2..=9).collect::<Vec<_>>());
    for n in 1..=9 {
        assert!(
            bucket_dir(&config)
                .join(format!("documents/{id}/versions/{n}.zip"))
                .exists()
        );
    }
}

/// [A version never changes](docs/boxes/viewer/server/constraints/a-version-never-changes.md):
/// the bucket keeps the upload byte for byte, the store its sha-256, and a
/// later publish leaves it as it was.
#[tokio::test]
async fn a_version_is_kept_byte_for_byte_and_never_changes() {
    let config = config("never-changes");
    let app = router(&config).unwrap();
    let first = site("Fixed", "one");
    let id = create(&app, first.clone()).await;
    let kept = bucket_dir(&config).join(format!("documents/{id}/versions/1.zip"));
    assert_eq!(std::fs::read(&kept).unwrap(), first);
    let (_, document) = ask(&app, get(&format!("/api/documents/{id}"))).await;
    assert_eq!(
        document["versions"][0]["sha256"],
        format!("{:x}", Sha256::digest(&first))
    );
    assert_eq!(document["versions"][0]["size"], first.len());
    let path = format!("/api/documents/{id}/versions");
    ask(&app, post(&path, site("Fixed", "two"))).await;
    assert_eq!(std::fs::read(&kept).unwrap(), first);
}

/// The first half of [the PVC is only a cache](docs/boxes/viewer/server/constraints/the-pvc-is-only-a-cache.md):
/// with the cache deleted, a server started again renders every version
/// from the bucket.
#[tokio::test]
async fn with_the_cache_deleted_every_version_renders_again_from_the_bucket() {
    let config = config("refill");
    let app = router(&config).unwrap();
    let id = create(&app, site("Kept", "one")).await;
    let path = format!("/api/documents/{id}/versions");
    ask(&app, post(&path, site("Kept", "two"))).await;
    for n in [1, 2] {
        assert_eq!(
            ask(&app, get(&format!("/d/{id}/v/{n}/README.md"))).await.0,
            StatusCode::OK
        );
    }
    drop(app);
    std::fs::remove_dir_all(config.data_dir.join("cache")).unwrap();
    let app = router(&config).unwrap();
    for (n, text) in [(1, "one"), (2, "two")] {
        let (status, html) = ask(&app, get(&format!("/d/{id}/v/{n}/README.md"))).await;
        assert_eq!(status, StatusCode::OK);
        assert!(html.as_str().unwrap().contains(text), "{html}");
    }
}

/// A server started again keeps what the cache held: served from disk,
/// without the bucket — and a publish leaves no scratch behind.
#[tokio::test]
async fn a_restarted_server_serves_what_its_cache_holds_without_the_bucket() {
    let config = config("seeded");
    let app = router(&config).unwrap();
    let id = create(&app, site("Seeded", "one")).await;
    assert_eq!(
        ask(&app, get(&format!("/d/{id}/v/1/README.md"))).await.0,
        StatusCode::OK
    );
    assert_eq!(
        std::fs::read_dir(config.data_dir.join("scratch"))
            .unwrap()
            .count(),
        0
    );
    drop(app);
    // Something half-unpacked by the last run goes.
    let partial = config.data_dir.join("cache").join(&id).join("7.partial-x");
    std::fs::create_dir_all(&partial).unwrap();
    std::fs::remove_file(bucket_dir(&config).join(format!("documents/{id}/versions/1.zip")))
        .unwrap();
    let app = router(&config).unwrap();
    assert!(!partial.exists());
    let (status, html) = ask(&app, get(&format!("/d/{id}/v/1/README.md"))).await;
    assert_eq!(status, StatusCode::OK, "{html}");
}

pub(super) fn cache(name: &str, limit: u64) -> Arc<Cache> {
    let dir = std::env::temp_dir().join(format!("devpage-cache-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Arc::new(Cache::open(&dir, limit, Box::new(Lru)).unwrap())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn misses_arriving_together_unpack_a_version_once() {
    let cache = cache("together", 1 << 20);
    let fetches = Arc::new(AtomicUsize::new(0));
    let makes = Arc::new(AtomicUsize::new(0));
    let zip = site("Once", "one");
    let misses = (0..8).map(|_| {
        let (cache, fetches, makes, zip) =
            (cache.clone(), fetches.clone(), makes.clone(), zip.clone());
        tokio::spawn(async move {
            let key = ("d".to_string(), 1);
            let fetch = async move {
                fetches.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                Ok(zip)
            };
            let served = cache
                .router(&key, "/d/d", fetch, |_| {
                    makes.fetch_add(1, Ordering::SeqCst);
                    Ok(Router::new())
                })
                .await
                .unwrap();
            drop(served);
        })
    });
    for miss in misses {
        miss.await.unwrap();
    }
    assert_eq!(fetches.load(Ordering::SeqCst), 1);
    assert_eq!(makes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_version_bigger_than_the_limit_is_served_without_being_cached() {
    let cache = cache("oversize", 10);
    let key = ("d".to_string(), 1);
    let seen = Arc::new(std::sync::Mutex::new(PathBuf::new()));
    let folder = seen.clone();
    let served = cache
        .router(
            &key,
            "/d/d",
            async { Ok(site("Big", "more than ten bytes")) },
            move |dir: &Path| {
                assert!(dir.join("README.md").is_file());
                *folder.lock().unwrap() = dir.to_path_buf();
                Ok(Router::new())
            },
        )
        .await
        .unwrap();
    assert!(!cache.holds(&key));
    assert_eq!(cache.size(), 0);
    let dir = seen.lock().unwrap().clone();
    assert!(dir.exists(), "there while it answers");
    drop(served);
    assert!(!dir.exists(), "deleted once it has");
}

#[tokio::test]
async fn a_version_answering_a_request_is_never_evicted() {
    let cache = cache("serving", 150);
    let make = |_: &Path| Ok(Router::new());
    let a = ("a".to_string(), 1);
    let b = ("b".to_string(), 1);
    let zip = || async { Ok(site("Sized", &"w".repeat(60))) };
    let holding_a = cache.router(&a, "/d/a", zip(), make).await.unwrap();
    let a_bytes = cache.size();
    assert!(a_bytes * 2 > 150 && a_bytes <= 150, "{a_bytes}");
    // b pushes the cache past its limit; a is the older, but answering.
    drop(cache.router(&b, "/d/b", zip(), make).await.unwrap());
    assert!(cache.holds(&a));
    drop(holding_a);
    // Once a isn't answering, the next insert lets it go.
    let c = ("c".to_string(), 1);
    drop(cache.router(&c, "/d/c", zip(), make).await.unwrap());
    assert!(!cache.holds(&a));
}

/// A publish names its document file with `?entry=`: a deck or a lone
/// Markdown file, not only a `devpage.yml`, checked and served from it.
#[tokio::test]
async fn a_publish_names_its_document_file_and_the_version_is_served_from_it() {
    let app = router(&config("entry")).unwrap();
    let zip = zip_of(&[
        ("notes.md", "# Notes\n\nThe notes.\n"),
        ("stray.md", "# Stray\n\n[gone](gone.md)\n"),
    ]);
    let (status, answer) = ask(&app, post("/api/documents?entry=notes.md", zip.clone())).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    let id = answer["id"].as_str().unwrap().to_string();
    let (status, html) = ask(&app, get(&format!("/d/{id}/notes.md"))).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.as_str().unwrap().contains("The notes."), "{html}");
    let (_, document) = ask(&app, get(&format!("/api/documents/{id}"))).await;
    assert_eq!(document["kind"], "doc");
    assert_eq!(document["title"], "Notes");

    // No `devpage.yml` in it: without an entry, there is no document.
    let (status, answer) = ask(&app, post("/api/documents", zip.clone())).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(answer.as_str().unwrap().contains("devpage.yml"), "{answer}");
    for entry in ["../notes.md", "/etc/passwd", "a/../notes.md", ""] {
        let (status, _) = ask(
            &app,
            post(&format!("/api/documents?entry={entry}"), zip.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{entry}");
    }
}

/// A published page lists no problem for an external address, and asks
/// none: external addresses are the CLI's to check.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_published_page_asks_no_external_address() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let app = router(&config("offline")).unwrap();
    let id = create(
        &app,
        site("Out", &format!("[away](http://{address}/missing)")),
    )
    .await;
    for _ in 0..3 {
        let (status, html) = ask(&app, get(&format!("/d/{id}/README.md"))).await;
        assert_eq!(status, StatusCode::OK);
        assert!(!html.as_str().unwrap().contains("missing —"), "{html}");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(listener.accept().is_err(), "nothing connected");
}

/// Evicting a version drops its routers under both mounts: the latest at
/// `/d/<id>` and itself at `/d/<id>/v/<n>` are both made again.
#[tokio::test]
async fn an_evicted_version_is_made_again_under_both_its_mounts() {
    let cache = cache("mounts", 150);
    let makes = Arc::new(AtomicUsize::new(0));
    let make = |makes: Arc<AtomicUsize>| {
        move |_: &Path| {
            makes.fetch_add(1, Ordering::SeqCst);
            Ok(Router::new())
        }
    };
    let zip = || async { Ok(site("Sized", &"w".repeat(60))) };
    let a = ("a".to_string(), 1);
    for mount in ["/d/a", "/d/a/v/1"] {
        drop(
            cache
                .router(&a, mount, zip(), make(makes.clone()))
                .await
                .unwrap(),
        );
    }
    assert_eq!(makes.load(Ordering::SeqCst), 2);
    // b pushes a out.
    let b = ("b".to_string(), 1);
    drop(
        cache
            .router(&b, "/d/b", zip(), make(makes.clone()))
            .await
            .unwrap(),
    );
    assert!(!cache.holds(&a));
    for mount in ["/d/a", "/d/a/v/1"] {
        drop(
            cache
                .router(&a, mount, zip(), make(makes.clone()))
                .await
                .unwrap(),
        );
    }
    assert_eq!(makes.load(Ordering::SeqCst), 5, "both made again");
}

#[tokio::test]
async fn a_versions_search_answers_from_the_index_its_first_search_builds() {
    let config = config("search-on-demand");
    let app = router(&config).unwrap();
    let id = create(&app, site("Field notes", "A walrus on the ice.")).await;
    let (status, _) = ask(&app, get(&format!("/d/{id}/README.md"))).await;
    assert_eq!(status, StatusCode::OK);
    for mount in [format!("/d/{id}"), format!("/d/{id}/v/1")] {
        let (status, found) = ask(&app, get(&format!("{mount}/__devpage/search?q=walrus"))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            found["pages"][0]["url"],
            format!("{mount}/README.md"),
            "found, under the mount it was asked under: {found}"
        );
        let (_, again) = ask(&app, get(&format!("{mount}/__devpage/search?q=walrus"))).await;
        assert_eq!(again, found, "the second search answers the same");
        let (_, none) = ask(&app, get(&format!("{mount}/__devpage/search?q=narwhal"))).await;
        assert_eq!(none, serde_json::json!({ "pages": [] }));
    }
}
