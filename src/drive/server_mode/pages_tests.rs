//! The server's own pages, a page's versions, downloads and live updates on
//! publish, without a browser; the browser's side is in
//! `server_mode_browser`.

use std::path::Path;
use std::time::Duration;

use axum::Router;
use axum::http::StatusCode;
use futures::StreamExt;
use tower::ServiceExt;

use super::router;
use super::tests::{ask, cache, config, create, delete, get, post, site, zip_of};

const BOX: &str = "<Frame width={40} height={40} fill=\"#2563eb\" />\n";

#[tokio::test]
async fn the_front_page_says_how_to_connect_and_lists_no_documents() {
    let app = router(&config("front")).unwrap();
    let id = create(&app, site("Secret plan", "hidden")).await;
    let request = axum::http::Request::get("/")
        .header("host", "docs.example.com")
        .body(axum::body::Body::empty())
        .unwrap();
    let (status, page) = ask(&app, request).await;
    assert_eq!(status, StatusCode::OK);
    let page = page.as_str().unwrap();
    assert!(page.contains("devpage server"), "{page}");
    assert!(
        page.contains("devpage context add work http://docs.example.com"),
        "{page}"
    );
    assert!(
        page.contains("href=\"https://github.com/andrei512/devpage\""),
        "{page}"
    );
    assert!(page.contains("<linearGradient id=\"bg\""), "{page}");
    assert!(!page.contains("{{"), "{page}");
    assert!(
        !page.contains(&id) && !page.contains("Secret plan"),
        "{page}"
    );
}

#[tokio::test]
async fn a_page_hears_which_version_it_shows_and_every_version_there_is() {
    let app = router(&config("versions")).unwrap();
    let id = create(&app, site("Plan", "one")).await;
    let (status, _) = ask(
        &app,
        post(
            &format!("/api/documents/{id}/versions"),
            site("Plan", "two"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // The latest, in place: not pinned, so no banner.
    let (status, latest) = ask(&app, get(&format!("/d/{id}/__devpage/versions"))).await;
    assert_eq!(status, StatusCode::OK, "{latest}");
    assert_eq!(latest["version"], 2);
    assert_eq!(latest["latest"], 2);
    assert_eq!(latest["pinned"], false);
    assert_eq!(latest["latest_url"], format!("/d/{id}/"));
    // A site has no download.
    assert!(latest["download"].is_null(), "{latest}");
    let numbers: Vec<u64> = latest["versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["number"].as_u64().unwrap())
        .collect();
    assert_eq!(numbers, [2, 1], "newest first");
    assert_eq!(latest["versions"][1]["url"], format!("/d/{id}/v/1/"));
    assert_eq!(latest["versions"][1]["author"], "anonymous");

    // Version 1, under its own mount: pinned, behind the latest.
    let (_, one) = ask(&app, get(&format!("/d/{id}/v/1/__devpage/versions"))).await;
    assert_eq!(one["version"], 1);
    assert_eq!(one["latest"], 2);
    assert_eq!(one["pinned"], true);

    // No such version, or document.
    let (status, _) = ask(&app, get(&format!("/d/{id}/v/9/__devpage/versions"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = ask(
        &app,
        get("/d/00000000-0000-0000-0000-000000000000/__devpage/versions"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// What `app` answers to a GET of `path`: status, headers and bytes.
async fn fetch(app: &Router, path: &str) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let response = app.clone().oneshot(get(path)).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, headers, body.to_vec())
}

#[tokio::test]
async fn a_designs_download_is_its_png_painted_once_and_kept_with_its_version() {
    let config = config("download-png");
    let app = router(&config).unwrap();
    let zip = zip_of(&[("box.design", BOX)]);
    let (status, answer) = ask(&app, post("/api/documents?entry=box.design", zip)).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    let id = answer["id"].as_str().unwrap().to_string();

    let (_, info) = ask(&app, get(&format!("/d/{id}/__devpage/versions"))).await;
    assert_eq!(info["download"]["url"], format!("/d/{id}/v/1/download"));
    assert_eq!(info["download"]["format"], "PNG");

    let (status, headers, png) = fetch(&app, &format!("/d/{id}/v/1/download")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["content-type"], "image/png");
    assert!(
        headers["content-disposition"]
            .to_str()
            .unwrap()
            .starts_with("attachment; filename="),
        "{headers:?}"
    );
    assert!(png.starts_with(b"\x89PNG"), "a PNG");
    let kept = config.data_dir.join(format!("cache/{id}/1.download.png"));
    assert_eq!(
        std::fs::read(&kept).unwrap(),
        png,
        "kept beside the version"
    );

    // The second is read from the cache: change what is kept, and that is
    // what comes back.
    std::fs::write(&kept, b"kept").unwrap();
    let (_, _, again) = fetch(&app, &format!("/d/{id}/v/1/download")).await;
    assert_eq!(again, b"kept");

    // Deleting the version takes its download with it.
    let (status, _) = ask(&app, delete(&format!("/api/documents/{id}/versions/1"))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(!kept.exists());
}

#[tokio::test]
async fn a_site_or_a_missing_version_has_no_download() {
    let app = router(&config("download-none")).unwrap();
    let id = create(&app, site("Plan", "one")).await;
    let (status, _, _) = fetch(&app, &format!("/d/{id}/v/1/download")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = fetch(&app, &format!("/d/{id}/v/2/download")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// A download is counted with its version, survives a restart counted the
/// same, and goes when the version is evicted.
#[tokio::test]
async fn a_download_counts_with_its_version_and_goes_with_it() {
    let cache = cache("downloads", 1 << 20);
    let key = ("d".to_string(), 1);
    let served = cache
        .router(
            &key,
            "/d/d/v/1",
            async { Ok(site("Sized", "words")) },
            |_: &Path| Ok(Router::new()),
        )
        .await
        .unwrap();
    let unpacked = cache.size();
    let rendered = cache
        .download(&served, "pdf", |folder| async move {
            assert!(folder.join("README.md").is_file());
            Ok(vec![7; 1000])
        })
        .await
        .unwrap();
    assert_eq!(rendered.len(), 1000);
    assert_eq!(cache.size(), unpacked + 1000);
    // Asked again, nothing renders.
    let again = cache
        .download(&served, "pdf", |_| async { panic!("rendered twice") })
        .await
        .unwrap();
    assert_eq!(again, rendered);
    drop(served);

    let dir = std::env::temp_dir().join(format!("devpage-cache-downloads-{}", std::process::id()));
    let reopened =
        super::cache::Cache::open(&dir, 1 << 20, Box::new(devpage_core::cache::Lru), 2000).unwrap();
    assert_eq!(reopened.size(), unpacked + 1000, "seeded with its download");
    reopened.remove(&key);
    assert!(!dir.join("d/1.download.pdf").exists());
}

/// The next message on an event stream, if one comes within `wait`.
async fn next_event(body: &mut axum::body::BodyDataStream, wait: Duration) -> Option<String> {
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        let chunk = tokio::time::timeout_at(deadline, body.next())
            .await
            .ok()??;
        let text = String::from_utf8_lossy(&chunk.ok()?).into_owned();
        if let Some(data) = text
            .lines()
            .find_map(|line| line.strip_prefix("data:").map(str::trim))
        {
            return Some(data.to_string());
        }
    }
}

async fn events(app: &Router, path: &str) -> axum::body::BodyDataStream {
    let response = app.clone().oneshot(get(path)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    response.into_body().into_data_stream()
}

#[tokio::test]
async fn a_publish_refreshes_pages_open_at_the_latest_and_never_an_old_versions() {
    let app = router(&config("live")).unwrap();
    let id = create(&app, site("Plan", "one")).await;
    let mut latest = events(&app, &format!("/d/{id}/__devpage/events")).await;
    let mut pinned = events(&app, &format!("/d/{id}/v/1/__devpage/events")).await;

    let (status, _) = ask(
        &app,
        post(
            &format!("/api/documents/{id}/versions"),
            site("Plan", "two"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let heard = next_event(&mut latest, Duration::from_secs(5)).await;
    assert_eq!(heard.as_deref(), Some("change"));
    assert_eq!(
        next_event(&mut pinned, Duration::from_millis(500)).await,
        None
    );

    // The page fetched again is version 2's — and a page that loaded it
    // still hears the next publish, through the same document's bus.
    let (_, _, page) = fetch(&app, &format!("/d/{id}/README.md")).await;
    assert!(String::from_utf8_lossy(&page).contains("two"));
    let mut on_two = events(&app, &format!("/d/{id}/__devpage/events")).await;
    let (status, _) = ask(
        &app,
        post(
            &format!("/api/documents/{id}/versions"),
            site("Plan", "three"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        next_event(&mut on_two, Duration::from_secs(5))
            .await
            .as_deref(),
        Some("change")
    );
    assert_eq!(
        next_event(&mut latest, Duration::from_secs(5))
            .await
            .as_deref(),
        Some("change")
    );

    // Deleting the latest version is a change to the latest too.
    let (status, _) = ask(&app, delete(&format!("/api/documents/{id}/versions/3"))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        next_event(&mut on_two, Duration::from_secs(5))
            .await
            .as_deref(),
        Some("change")
    );
}

#[tokio::test]
async fn a_design_resolves_under_its_documents_mount() {
    let app = router(&config("design-mount")).unwrap();
    let id = create(
        &app,
        zip_of(&[
            ("devpage.yml", "kind: site\ntitle: T\nroot: README.md\n"),
            ("README.md", "# T\n\n[The box](designs/box.design)\n"),
            ("designs/box.design", BOX),
        ]),
    )
    .await;
    for path in [
        format!("/d/{id}/README.md"),
        format!("/d/{id}/v/1/README.md"),
    ] {
        let (status, page) = ask(&app, get(&path)).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        let page = page.as_str().unwrap();
        assert!(!page.contains("broken design"), "{path}: {page}");
    }
}

#[tokio::test]
async fn a_pages_problems_are_found_on_its_first_view_and_kept_with_its_version() {
    let config = config("problems-once");
    let app = router(&config).unwrap();
    // A page the document doesn't reach, so the publish doesn't check it.
    let stray = "# Stray\n\n[gone](gone.md) and [heading](README.md#nope)\n";
    let doc = |stray: &str| {
        zip_of(&[
            (
                "devpage.yml",
                "kind: doc\ntitle: T\nchapters:\n  - README.md\n",
            ),
            ("README.md", "# T\n"),
            ("stray.md", stray),
        ])
    };
    let id = create(&app, doc(stray)).await;
    let (status, _) = ask(
        &app,
        post(&format!("/api/documents/{id}/versions"), doc("# Stray\n")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let shown = |page: &serde_json::Value| {
        let page = page.as_str().unwrap();
        (
            page.contains("broken link gone.md"),
            page.contains("no heading #nope"),
        )
    };
    let first = ask(&app, get(&format!("/d/{id}/v/1/stray.md"))).await.1;
    assert_eq!(shown(&first), (true, true), "{first}");
    // Fixed in the cache behind the server's back, which a version never is:
    // every later view shows what the first found, so none checked again.
    let cached = config
        .data_dir
        .join("cache")
        .join(&id)
        .join("1")
        .join("stray.md");
    std::fs::write(&cached, "# Stray\n").unwrap();
    for _ in 0..9 {
        let (_, page) = ask(&app, get(&format!("/d/{id}/v/1/stray.md"))).await;
        assert_eq!(shown(&page), (true, true), "{page}");
    }
    // Version 2, the latest, keeps its own: the link it fixed shows nowhere.
    for path in [format!("/d/{id}/v/2/stray.md"), format!("/d/{id}/stray.md")] {
        let (_, page) = ask(&app, get(&path)).await;
        assert_eq!(shown(&page), (false, false), "{path}: {page}");
    }
}

#[tokio::test]
async fn the_tab_icon_is_devpages_logo() {
    let app = router(&config("favicon")).unwrap();
    let response = app.oneshot(get("/favicon.ico")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/x-icon");
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(body.starts_with(&[0, 0, 1, 0]), "an ICO");
    assert_eq!(&body[22..26], b"\x89PNG", "holding a PNG");
}

#[test]
fn the_servers_logo_is_devpages() {
    assert_eq!(
        include_str!("../../../assets/logo.svg"),
        include_str!("../../../docs/logo.svg"),
        "assets/logo.svg is a copy of docs/logo.svg: copy it again, and redraw assets/favicon.ico"
    );
}
