//! A published document in chrome-headless-shell: every request its pages
//! make stays under their mount. Run with `cargo test -- --ignored`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::Request;
use axum::middleware::Next;
use chromiumoxide::Page;
use chromiumoxide::browser::Browser;
use futures::StreamExt;

use super::server_mode::{Config, router};
use crate::pdf::Profile;

/// Every request the server answered, by path, with its status.
type Heard = Arc<Mutex<Vec<(String, u16)>>>;

/// A zip of `files`, as `devpage publish` sends one.
fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
    use std::io::Write;
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, text) in files {
        out.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        out.write_all(text.as_bytes()).unwrap();
    }
    out.finish().unwrap().into_inner()
}

/// A site with everything a page asks its server for: a sidebar with
/// search, a navbar, a system widget, a project widget that renders one, an
/// embedded design and the live-update stream.
fn site() -> Vec<u8> {
    zip_of(&[
        (
            "devpage.yml",
            "kind: site\ntitle: Launch\nroot: README.md\nwidgets: widgets\nnavbar:\n  Guide: guide.md\n",
        ),
        (
            "README.md",
            "# Launch\n\n<Callout>Read the guide.</Callout>\n\n<Badge />\n\n![The box](app/box.design)\n\n[The guide](guide.md)\n",
        ),
        ("guide.md", "# Guide\n\nThe rollout, step by step.\n"),
        (
            "widgets/Badge.tsx",
            "import { label } from \"./label\";\nexport default () => <span id=\"badge\"><Callout>{label}</Callout></span>;\n",
        ),
        ("widgets/label.ts", "export const label = \"shipped\";\n"),
        (
            "app/box.design",
            "<Frame width={40} height={40} fill=\"#2563eb\" />\n",
        ),
    ])
}

/// `devpage server` on a free port, against a bucket in a folder, recording
/// every request it answers.
async fn start() -> (String, Heard) {
    let dir = std::env::temp_dir().join(format!("devpage-mounted-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("bucket")).unwrap();
    let config = Config::local(&dir.join("data"), &dir.join("bucket"), 1 << 30);
    let heard = Heard::default();
    let recording = heard.clone();
    let app = router(&config).unwrap().layer(axum::middleware::from_fn(
        move |request: Request, next: Next| {
            let heard = recording.clone();
            async move {
                let path = request.uri().path().to_string();
                let response = next.run(request).await;
                heard
                    .lock()
                    .unwrap()
                    .push((path, response.status().as_u16()));
                response
            }
        },
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.ok() });
    (format!("http://{address}"), heard)
}

async fn eval<T: serde::de::DeserializeOwned>(page: &Page, js: &str) -> T {
    page.evaluate(js).await.unwrap().into_value().unwrap()
}

/// Waits until `js` is true, for up to 20 seconds.
async fn until(page: &Page, js: &str) {
    for _ in 0..1000 {
        if page
            .evaluate(js)
            .await
            .ok()
            .and_then(|v| v.into_value::<bool>().ok())
            .unwrap_or(false)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("never true: {js}");
}

/// Whether `path` is a built-in asset's address: the same for every
/// document, so outside any mount.
fn built_in(path: &str) -> bool {
    path.strip_prefix("/__devpage/").is_some_and(|name| {
        devpage_core::addresses::asset(name).is_some_and(|(_, current)| current)
    })
}

/// Opens `mount`'s root page and uses everything on it, then checks that
/// every request the page made was answered, under `mount`.
async fn every_request_stays_under(browser: &Browser, base: &str, heard: &Heard, mount: &str) {
    heard.lock().unwrap().clear();
    let page = browser
        .new_page(format!("{base}{mount}/README.md").as_str())
        .await
        .unwrap();
    until(
        &page,
        "document.readyState === 'complete' && document.documentElement.dataset.ready === 'true'",
    )
    .await;
    assert_eq!(
        eval::<String>(&page, "document.documentElement.dataset.mount").await,
        mount
    );
    // The project widget, the helper it imports and the system widget it
    // renders; the embedded design, drawn through its watch stream.
    until(
        &page,
        "!!document.querySelector('#badge')?.textContent.includes('shipped')",
    )
    .await;
    until(&page, "document.querySelectorAll('.callout').length === 2").await;
    until(
        &page,
        "document.querySelector('canvas.design')?.dataset.painted === 'true'",
    )
    .await;
    // Search, from the sidebar's box: its hits lead inside the mount.
    page.find_element("input[type=search]")
        .await
        .unwrap()
        .click()
        .await
        .unwrap()
        .type_str("rollout")
        .await
        .unwrap();
    until(&page, "!!document.querySelector('.search-results a')").await;
    let hit: String = eval(
        &page,
        "new URL(document.querySelector('.search-results a').href).pathname",
    )
    .await;
    assert_eq!(hit, format!("{mount}/guide.md"));
    // Every link the sidebar and the navbar carry is under the mount, and
    // answers.
    let links: Vec<String> = eval(
        &page,
        "[...document.querySelectorAll('#devpage-nav a[href], #devpage-navbar a[href], #devpage-page a[href]')]
           .map((a) => new URL(a.href)).filter((u) => u.origin === location.origin)
           .map((u) => u.pathname)",
    )
    .await;
    assert!(links.iter().any(|l| l.ends_with("/guide.md")), "{links:?}");
    for link in &links {
        assert!(
            link.starts_with(&format!("{mount}/")),
            "{link} in {links:?}"
        );
        let status = reqwest::get(format!("{base}{link}"))
            .await
            .unwrap()
            .status();
        assert_eq!(status, 200, "{link}");
    }
    // The live-update stream is open, under the mount.
    let events = format!("{mount}/__devpage/events");
    for _ in 0..500 {
        if heard.lock().unwrap().iter().any(|(p, _)| *p == events) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // The embed links to its canvas page under the mount, and that page
    // draws the design through the same stream.
    let canvas: String = eval(
        &page,
        "new URL(document.querySelector('.design-embed a[href]').href).pathname",
    )
    .await;
    assert_eq!(canvas, format!("{mount}/app/box.design"));
    page.close().await.unwrap();
    let page = browser
        .new_page(format!("{base}{canvas}").as_str())
        .await
        .unwrap();
    until(
        &page,
        "document.querySelector('body > canvas.design')?.dataset.painted === 'true'",
    )
    .await;
    page.close().await.unwrap();
    let heard = heard.lock().unwrap().clone();
    assert!(heard.iter().any(|(p, _)| *p == events), "{heard:#?}");
    for (path, status) in &heard {
        assert!(
            path.starts_with(&format!("{mount}/")) || built_in(path),
            "{path} is outside {mount}: {heard:#?}"
        );
        assert!(*status < 400, "{path} answered {status}: {heard:#?}");
    }
    for asked in [
        "/__devpage/widgets/",
        "/__devpage/system-widgets/",
        "/devpage.design.v1.Designs/Watch",
    ] {
        assert!(
            heard
                .iter()
                .any(|(p, _)| p.starts_with(&format!("{mount}{asked}"))),
            "nothing asked {mount}{asked}: {heard:#?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "drives Chrome"]
async fn a_published_document_asks_only_under_its_mount() {
    let (base, heard) = start().await;
    let response = reqwest::Client::new()
        .post(format!("{base}/api/documents"))
        .body(site())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    let answer: serde_json::Value = response.json().await.unwrap();
    let id = answer["id"].as_str().unwrap().to_string();

    let profile = Profile::new();
    let config = profile.chrome().await.unwrap().build().unwrap();
    let (browser, mut handler) = Browser::launch(config).await.unwrap();
    let events = tokio::spawn(async move { while handler.next().await.is_some() {} });

    // The latest, in place at the link, and the same version at its own
    // address.
    every_request_stays_under(&browser, &base, &heard, &format!("/d/{id}")).await;
    every_request_stays_under(&browser, &base, &heard, &format!("/d/{id}/v/1")).await;

    drop(browser);
    events.abort();
}
