//! Comments on a published document in chrome-headless-shell: a thread
//! made on the page is the document's, one made through the API reaches the
//! open page, and a publish that removes a thread's text lists it detached.
//! Run with `cargo test -- --ignored`.

use std::time::Duration;

use chromiumoxide::Page;
use chromiumoxide::browser::Browser;
use futures::StreamExt;
use serde_json::{Value, json};

use super::server_mode::{Config, router};
use crate::pdf::Profile;

fn site(text: &str) -> Vec<u8> {
    use std::io::Write;
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in [
        (
            "devpage.yml",
            "kind: site\ntitle: Launch\nroot: README.md\n".to_string(),
        ),
        ("README.md", format!("# Launch\n\n{text}\n")),
    ] {
        out.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        out.write_all(body.as_bytes()).unwrap();
    }
    out.finish().unwrap().into_inner()
}

/// `devpage server` on a free port, against a bucket in a folder.
async fn start() -> String {
    let dir = std::env::temp_dir().join(format!("devpage-commented-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("bucket")).unwrap();
    let config = Config::local(&dir.join("data"), &dir.join("bucket"), 1 << 30);
    let app = router(&config).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.ok() });
    format!("http://{address}")
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

async fn loaded(page: &Page) {
    until(
        page,
        "document.readyState === 'complete' && document.documentElement.dataset.ready === 'true'",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "drives Chrome"]
async fn a_published_documents_threads_are_its_own_across_the_page_the_api_and_a_publish() {
    let base = start().await;
    let http = reqwest::Client::new();
    let published: Value = http
        .post(format!("{base}/api/documents"))
        .body(site(
            "The client retries every write until the server acknowledges it.",
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = published["id"].as_str().unwrap().to_string();

    let profile = Profile::new();
    let config = profile.chrome().await.unwrap().build().unwrap();
    let (browser, mut handler) = Browser::launch(config).await.unwrap();
    let events = tokio::spawn(async move { while handler.next().await.is_some() {} });
    let page = browser
        .new_page(format!("{base}/d/{id}/README.md").as_str())
        .await
        .unwrap();
    loaded(&page).await;

    // A thread made on the page, as anywhere: comment mode, a selection,
    // the composer.
    eval::<bool>(
        &page,
        "(document.querySelector('.comment-mode').click(), true)",
    )
    .await;
    eval::<bool>(
        &page,
        "(() => { const p = document.querySelector('article p'); const t = p.firstChild;
           const at = t.data.indexOf('retries every write');
           const r = document.createRange(); r.setStart(t, at); r.setEnd(t, at + 19);
           getSelection().removeAllRanges(); getSelection().addRange(r);
           p.dispatchEvent(new MouseEvent('mouseup', { bubbles: true })); return true; })()",
    )
    .await;
    until(&page, "!!document.querySelector('.comment-button')").await;
    eval::<bool>(
        &page,
        "document.querySelector('.comment-button').click(), true",
    )
    .await;
    until(
        &page,
        "!!document.querySelector('#devpage-comments .composer textarea')",
    )
    .await;
    eval::<bool>(
        &page,
        "(() => { const box = document.querySelector('#devpage-comments .composer textarea');
           box.value = 'Every write, or only the outbox?';
           box.closest('form').requestSubmit(); return true; })()",
    )
    .await;
    until(
        &page,
        "document.querySelector('mark.comment')?.textContent === 'retries every write'",
    )
    .await;

    // It is the document's: the API lists it, on the page's own path.
    let threads: Vec<Value> = http
        .get(format!("{base}/api/documents/{id}/comments"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(threads.len(), 1, "{threads:?}");
    assert_eq!(threads[0]["page"], "/README.md");
    assert_eq!(threads[0]["version"], 1);
    assert_eq!(
        threads[0]["thread"][0]["text"],
        "Every write, or only the outbox?"
    );

    // A thread an agent starts through the API reaches the open page.
    let started = http
        .post(format!("{base}/api/documents/{id}/comments"))
        .json(&json!({
            "page": "/README.md",
            "anchor": { "quote": "acknowledges it", "prefix": "the server ", "suffix": "." },
            "text": "Acknowledged how?",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(started.status(), 201);
    until(
        &page,
        "[...document.querySelectorAll('mark.comment')].some((m) => m.textContent === 'acknowledges it')",
    )
    .await;

    // Version 2 drops the second quote: on the latest, that thread is
    // listed detached, the first still marked in the text.
    let updated = http
        .post(format!("{base}/api/documents/{id}/versions"))
        .body(site("The client retries every write until a peer replies."))
        .send()
        .await
        .unwrap();
    assert_eq!(updated.status(), 201);
    page.reload().await.unwrap();
    loaded(&page).await;
    until(&page, "!!document.querySelector('.detached-heading')").await;
    let detached: String = eval(
        &page,
        "document.querySelector('.detached-heading').nextElementSibling.textContent",
    )
    .await;
    assert!(detached.contains("Acknowledged how?"), "{detached}");
    until(
        &page,
        "document.querySelector('mark.comment')?.textContent === 'retries every write'",
    )
    .await;

    drop(browser);
    events.abort();
}
