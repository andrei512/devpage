//! Comments on a published document in chrome-headless-shell: a thread
//! made on the page is the document's, one made through the API reaches the
//! open page, and a publish that removes a thread's text lists it detached.

use serde_json::{Value, json};

use super::{publish, server_mode, zip_of};
use crate::chrome::testing::{self, eval, until};

fn site(text: &str) -> Vec<u8> {
    zip_of(&[
        (
            "devpage.yml",
            "kind: site\ntitle: Launch\nroot: README.md\n",
        ),
        ("README.md", &format!("# Launch\n\n{text}\n")),
    ])
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "drives Chrome"]
async fn a_published_documents_threads_are_its_own_across_the_page_the_api_and_a_publish() {
    let base = testing::serve(server_mode("comments")).await;
    let http = reqwest::Client::new();
    let id = publish(
        &base,
        None,
        site("The client retries every write until the server acknowledges it."),
    )
    .await;

    let browser = testing::launch().await;
    let page = testing::open(&browser, &format!("{base}/d/{id}/README.md")).await;

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
    publish(
        &base,
        Some(&id),
        site("The client retries every write until a peer replies."),
    )
    .await;
    page.reload().await.unwrap();
    testing::loaded(&page).await;
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
}
