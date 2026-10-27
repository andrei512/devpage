//! A published document's pages in chrome-headless-shell: a page open at the
//! link swaps in each new version, an old version says so and links to the
//! latest, and the version menu and the download button. Run with
//! `cargo test -- --ignored`.

use std::time::Duration;

use chromiumoxide::Page;
use chromiumoxide::browser::Browser;
use futures::StreamExt;

use super::server_mode::{Config, router};
use crate::pdf::Profile;

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

fn doc(text: &str) -> Vec<u8> {
    zip_of(&[
        (
            "devpage.yml",
            "kind: doc\ntitle: Rollout\nchapters:\n  - rollout.md\n",
        ),
        ("rollout.md", &format!("# Rollout\n\n{text}\n")),
    ])
}

/// `devpage server` on a free port, against a bucket in a folder.
async fn start(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("devpage-pages-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("bucket")).unwrap();
    let config = Config::local(&dir.join("data"), &dir.join("bucket"), 1 << 30);
    let app = router(&config).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.ok() });
    format!("http://{address}")
}

/// Publishes `zip` — to document `id`, or as a new one — and answers its id.
async fn publish(base: &str, id: Option<&str>, zip: Vec<u8>) -> String {
    let url = match id {
        Some(id) => format!("{base}/api/documents/{id}/versions"),
        None => format!("{base}/api/documents"),
    };
    let response = reqwest::Client::new()
        .post(url)
        .body(zip)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    let answer: serde_json::Value = response.json().await.unwrap();
    id.map(str::to_string)
        .unwrap_or_else(|| answer["id"].as_str().unwrap().to_string())
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

async fn open(browser: &Browser, url: &str) -> Page {
    let page = browser.new_page(url).await.unwrap();
    until(
        &page,
        "document.readyState === 'complete' && document.documentElement.dataset.ready === 'true'",
    )
    .await;
    page
}

async fn chrome() -> (Browser, tokio::task::JoinHandle<()>) {
    let profile = Profile::new();
    let config = profile.chrome().await.unwrap().build().unwrap();
    let (browser, mut handler) = Browser::launch(config).await.unwrap();
    let events = tokio::spawn(async move { while handler.next().await.is_some() {} });
    (browser, events)
}

const TEXT: &str = "document.getElementById('devpage-page').textContent";
const MENU: &str = "document.querySelector('#devpage-versions summary')?.textContent";

#[tokio::test(flavor = "multi_thread")]
#[ignore = "drives Chrome"]
async fn a_page_open_at_the_link_swaps_in_each_new_version() {
    let base = start("live").await;
    let id = publish(&base, None, doc("The first plan.")).await;
    let (browser, events) = chrome().await;
    let page = open(&browser, &format!("{base}/d/{id}/")).await;
    until(&page, &format!("{MENU} === 'Version 1 · latest'")).await;
    assert!(
        eval::<String>(&page, TEXT)
            .await
            .contains("The first plan.")
    );
    // The link leads to the doc's address, still under the document.
    let at: String = eval(&page, "location.pathname").await;
    assert_eq!(at, format!("/d/{id}/devpage.yml"));

    publish(&base, Some(&id), doc("The second plan.")).await;
    until(&page, &format!("{TEXT}.includes('The second plan.')")).await;
    until(&page, &format!("{MENU} === 'Version 2 · latest'")).await;
    // Swapped in place, at the same address: the latest, not version 2's.
    assert_eq!(eval::<String>(&page, "location.pathname").await, at);
    // And the next publish reaches it too.
    publish(&base, Some(&id), doc("The third plan.")).await;
    until(&page, &format!("{TEXT}.includes('The third plan.')")).await;
    assert!(
        eval::<bool>(&page, "!document.getElementById('devpage-version-banner')").await,
        "the latest has no banner"
    );
    drop(browser);
    events.abort();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "drives Chrome"]
async fn an_old_version_says_so_never_changes_and_lists_every_version() {
    let base = start("old").await;
    let id = publish(&base, None, doc("The first plan.")).await;
    publish(&base, Some(&id), doc("The second plan.")).await;
    let (browser, events) = chrome().await;
    let page = open(&browser, &format!("{base}/d/{id}/v/1/")).await;
    until(&page, "!!document.getElementById('devpage-version-banner')").await;
    let banner: String = eval(
        &page,
        "document.getElementById('devpage-version-banner').textContent",
    )
    .await;
    assert_eq!(
        banner,
        "You are reading version 1; the latest is version 2."
    );
    let latest: String = eval(
        &page,
        "new URL(document.querySelector('#devpage-version-banner a').href).pathname",
    )
    .await;
    assert_eq!(latest, format!("/d/{id}/"));
    assert_eq!(eval::<String>(&page, MENU).await, "Version 1");
    let listed: Vec<String> = eval(
        &page,
        "[...document.querySelectorAll('.version-menu a')].map((a) => new URL(a.href).pathname)",
    )
    .await;
    assert_eq!(listed, [format!("/d/{id}/v/2/"), format!("/d/{id}/v/1/")]);
    let current: String = eval(
        &page,
        "document.querySelector('.version-menu a[aria-current]').textContent",
    )
    .await;
    assert_eq!(current, "Version 1");

    // A publish leaves version 1 as it is.
    publish(&base, Some(&id), doc("The third plan.")).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let text: String = eval(&page, TEXT).await;
    assert!(text.contains("The first plan."), "{text}");
    drop(browser);
    events.abort();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "drives Chrome"]
async fn a_docs_download_button_saves_its_pdf_printed_once() {
    let base = start("pdf").await;
    let id = publish(&base, None, doc("The plan, on paper.")).await;
    let (browser, events) = chrome().await;
    let page = open(&browser, &format!("{base}/d/{id}/")).await;
    until(&page, "!!document.querySelector('.version-download')").await;
    assert_eq!(
        eval::<String>(
            &page,
            "document.querySelector('.version-download').textContent"
        )
        .await,
        "Download PDF"
    );
    let href: String = eval(
        &page,
        "new URL(document.querySelector('.version-download').href).pathname",
    )
    .await;
    assert_eq!(href, format!("/d/{id}/v/1/download"));
    drop(browser);
    events.abort();

    let first = std::time::Instant::now();
    let response = reqwest::get(format!("{base}{href}")).await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["content-type"], "application/pdf");
    assert_eq!(
        response.headers()["content-disposition"],
        "attachment; filename=\"Rollout.pdf\""
    );
    let pdf = response.bytes().await.unwrap();
    assert!(pdf.starts_with(b"%PDF"), "a PDF");
    let printed = first.elapsed();
    let second = std::time::Instant::now();
    let again = reqwest::get(format!("{base}{href}"))
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(again, pdf, "the same PDF, from the cache");
    assert!(
        second.elapsed() * 5 < printed,
        "read, not printed again: {:?} after {printed:?}",
        second.elapsed()
    );
}
