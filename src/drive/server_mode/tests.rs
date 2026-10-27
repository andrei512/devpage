use std::io::Write;
use std::path::Path;

use axum::body::Body;
use axum::extract::Request;
use axum::http::StatusCode;
use tower::ServiceExt;

use super::*;

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
    let dir = std::env::temp_dir().join(format!("devpage-server-{name}-{}", std::process::id()));
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
