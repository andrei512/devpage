//! The page script's side of comments, in chrome-headless-shell. Run with
//! `cargo test -- --ignored`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chromiumoxide::browser::{Browser, BrowserConfig};
use futures::StreamExt;

use crate::comments::*;
use crate::document::Document;
use crate::server;

/// Serves `entry` from `root`, opens `path` in a headless Chrome with a
/// profile of its own and turns comment mode on, as a reader starting a
/// comment does.
async fn open(
    name: &str,
    root: &Path,
    entry: &Path,
    path: &str,
) -> (Browser, chromiumoxide::Page, tokio::task::JoinHandle<()>) {
    let opened = open_off(name, root, entry, path).await;
    eval::<bool>(
        &opened.1,
        "(document.querySelector('.comment-mode').click(), true)",
    )
    .await;
    opened
}

/// The same, with comment mode as a first visit finds it: off.
async fn open_off(
    name: &str,
    root: &Path,
    entry: &Path,
    path: &str,
) -> (Browser, chromiumoxide::Page, tokio::task::JoinHandle<()>) {
    let document = Arc::new(Document::load(entry, root).unwrap());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let changes = crate::changes::Changes::default();
    let index = Arc::new(crate::index::Index::build(&document.served));
    let entry = Arc::new(crate::document::Entry::new(document));
    let designs = crate::designs::Designs::new(entry.document().served.path().to_path_buf());
    let document = entry.document();
    let options = server::Options {
        entry,
        index,
        designs,
        comments: Some(server::Identity::Local("Ana".into())),
        preview: crate::check::testing::preview(),
        ..server::Options::new(document, changes.clone())
    };
    let _watcher = options.watch().unwrap();
    let router = server::router(options);
    tokio::spawn(async move {
        let _watcher = _watcher;
        axum::serve(listener, router).await.ok();
    });
    let profile = std::env::temp_dir().join(format!(
        "devpage-chrome-comments-{name}-{}",
        std::process::id()
    ));
    let config = BrowserConfig::builder()
        .chrome_executable(crate::pdf::headless_shell().await.unwrap())
        .user_data_dir(profile)
        .viewport(chromiumoxide::handler::viewport::Viewport {
            width: 1400,
            height: 900,
            ..Default::default()
        })
        .build()
        .unwrap();
    let (browser, mut handler) = Browser::launch(config).await.unwrap();
    let events = tokio::spawn(async move { while handler.next().await.is_some() {} });
    let page = browser
        .new_page(format!("http://{address}{path}").as_str())
        .await
        .unwrap();
    loaded(&page).await;
    (browser, page, events)
}

/// Waits until `page` has loaded and its script has finished starting —
/// widgets mounted, comments asked for — however long a loaded machine
/// takes, rather than for a fixed while; for up to 20 seconds.
async fn loaded(page: &chromiumoxide::Page) {
    for _ in 0..1000 {
        let ready = page
            .evaluate(
                "document.readyState === 'complete' \
                 && document.documentElement.dataset.ready === 'true'",
            )
            .await
            .ok()
            .and_then(|v| v.into_value::<bool>().ok())
            .unwrap_or(false);
        if ready {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the page never finished loading");
}

async fn eval<T: serde::de::DeserializeOwned>(page: &chromiumoxide::Page, js: &str) -> T {
    page.evaluate(js).await.unwrap().into_value().unwrap()
}

/// Polls `js` until it returns true, for up to four seconds.
async fn until(page: &chromiumoxide::Page, js: &str) -> bool {
    for _ in 0..40 {
        if eval::<bool>(page, js).await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

fn folder(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("devpage-browser-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[tokio::test]
#[ignore]
async fn commenting_is_off_until_comment_mode_is_on_and_stays_on_after_a_reload() {
    let root = folder("mode");
    std::fs::write(root.join("a.md"), "# A\n\nA paragraph to comment on.\n").unwrap();
    let (mut browser, page, events) = open_off("mode", &root, &root.join("a.md"), "/a.md").await;
    let hover = "(document.querySelector('article p').dispatchEvent(new MouseEvent('mouseover', { bubbles: true })), true)";
    let pressed = "document.querySelector('.comment-mode').getAttribute('aria-pressed')";

    // Off: hovering offers nothing, and the switch is not pressed.
    eval::<bool>(&page, hover).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!eval::<bool>(&page, "!!document.querySelector('.comment-button')").await);
    assert_eq!(eval::<String>(&page, pressed).await, "false");

    // On: the icon appears beside the paragraph, and a comment can be made.
    eval::<bool>(
        &page,
        "(document.querySelector('.comment-mode').click(), true)",
    )
    .await;
    assert_eq!(eval::<String>(&page, pressed).await, "true");
    eval::<bool>(&page, "(document.querySelector('article h1').dispatchEvent(new MouseEvent('mouseover', { bubbles: true })), true)").await;
    eval::<bool>(&page, hover).await;
    assert!(until(&page, "!!document.querySelector('.comment-button')").await);
    eval::<bool>(
        &page,
        "(document.querySelector('.comment-button').click(), true)",
    )
    .await;
    assert!(
        until(
            &page,
            "!!document.querySelector('#devpage-comments .composer textarea')"
        )
        .await
    );
    eval::<bool>(
        &page,
        "(() => { const box = document.querySelector('#devpage-comments .composer textarea');
           box.value = 'Is this the right paragraph?';
           box.closest('form').requestSubmit(); return true; })()",
    )
    .await;
    assert!(until(&page, "!!document.querySelector('mark.comment')").await);

    // Remembered: after a reload the switch is still on, and turning it off
    // removes the icon while the thread stays.
    page.reload().await.unwrap();
    loaded(&page).await;
    assert_eq!(eval::<String>(&page, pressed).await, "true");
    eval::<bool>(
        &page,
        "(document.querySelector('.comment-mode').click(), true)",
    )
    .await;
    eval::<bool>(&page, hover).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!eval::<bool>(&page, "!!document.querySelector('.comment-button')").await);
    assert!(
        until(
            &page,
            "!!document.querySelector('#devpage-comments .thread')"
        )
        .await
    );

    browser.close().await.ok();
    events.abort();
}

#[tokio::test]
#[ignore]
async fn selected_text_becomes_a_thread_in_the_margin_and_in_the_store() {
    let root = folder("select");
    std::fs::write(
        root.join("a.md"),
        "# A\n\nThe client retries every write until the server acknowledges it.\n",
    )
    .unwrap();
    let (mut browser, page, events) = open("select", &root, &root.join("a.md"), "/a.md").await;

    // Select "retries every write" and release the mouse.
    eval::<bool>(
        &page,
        "(() => { const p = document.querySelector('article p'); const t = p.firstChild;
           const at = t.data.indexOf('retries every write');
           const r = document.createRange(); r.setStart(t, at); r.setEnd(t, at + 19);
           getSelection().removeAllRanges(); getSelection().addRange(r);
           p.dispatchEvent(new MouseEvent('mouseup', { bubbles: true })); return true; })()",
    )
    .await;
    assert!(until(&page, "!!document.querySelector('.comment-button')").await);
    eval::<bool>(
        &page,
        "document.querySelector('.comment-button').click(), true",
    )
    .await;
    assert!(
        until(
            &page,
            "!!document.querySelector('#devpage-comments .composer textarea')"
        )
        .await
    );
    eval::<bool>(
        &page,
        "(() => { const box = document.querySelector('#devpage-comments .composer textarea');
           box.value = 'Every write, or only the outbox?';
           box.closest('form').requestSubmit(); return true; })()",
    )
    .await;
    assert!(
        until(
            &page,
            "document.querySelector('mark.comment')?.textContent === 'retries every write'"
        )
        .await
    );
    let layout: String = eval(
        &page,
        "innerWidth + ' ' + document.getElementById('devpage-comments').className
         + ' ' + !!document.querySelector('#devpage-comments .composer')",
    )
    .await;
    assert_eq!(
        layout, "1400 comment-ui beside false",
        "the thread sits beside the text and the composer closed"
    );

    let threads = yaml::Yaml::new(&root).list("/a.md").unwrap();
    assert_eq!(threads.len(), 1);
    let thread = &threads[0].thread;
    assert_eq!(thread.anchor.quote, "retries every write");
    assert_eq!(
        thread.anchor.prefix, "A The client ",
        "up to 32 characters of the text before, headings included"
    );
    assert_eq!(thread.anchor.suffix, " until the server acknowledges i");
    assert_eq!(thread.thread[0].author, "Ana");
    assert_eq!(thread.thread[0].text, "Every write, or only the outbox?");

    // Rewrite the sentence: the thread is detached, not lost.
    std::fs::write(root.join("a.md"), "# A\n\nThe client gives up.\n").unwrap();
    assert!(
        until(
            &page,
            "(document.getElementById('devpage-comments')?.textContent ?? '').includes('Detached comments')
             && !document.querySelector('mark.comment')"
        )
        .await
    );
    browser.close().await.ok();
    events.abort();
}

#[tokio::test]
#[ignore]
async fn the_paragraph_comment_icon_stays_inside_narrow_windows() {
    use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
    let root = folder("narrow");
    std::fs::write(
        root.join("a.md"),
        "# A\n\nA paragraph long enough to reach the right edge of a narrow window, and then some more words.\n\n| a | b | c | d | e |\n| --- | --- | --- | --- | --- |\n| a_rather_long_cell_value | another_rather_long_cell | a_third_long_cell_value | yet_another_long_cell | the_last_long_cell_value |\n",
    )
    .unwrap();
    let (mut browser, page, events) = open("narrow", &root, &root.join("a.md"), "/a.md").await;
    for width in [390, 820] {
        page.execute(SetDeviceMetricsOverrideParams::new(width, 800, 1.0, false))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        // Hover the heading, then the paragraph, so the paragraph counts
        // as newly hovered at each width.
        eval::<bool>(
            &page,
            "(document.querySelector('article h1').dispatchEvent(new MouseEvent('mouseover', { bubbles: true })),
              document.querySelector('.comment-button')?.remove(),
              document.querySelector('article p').dispatchEvent(new MouseEvent('mouseover', { bubbles: true })), true)",
        )
        .await;
        assert!(until(&page, "!!document.querySelector('.comment-button')").await);
        let fits: String = eval(
            &page,
            "(() => { const b = document.querySelector('.comment-button').getBoundingClientRect();
               const w = document.documentElement.clientWidth;
               return [b.left >= 0, b.right <= w, document.documentElement.scrollWidth <= w].join(' '); })()",
        )
        .await;
        assert_eq!(
            fits, "true true true",
            "at {width}px: icon left, icon right, no page scroll"
        );
    }
    browser.close().await.ok();
    events.abort();
}

#[tokio::test]
#[ignore]
async fn a_paragraph_comment_on_a_nested_list_item_anchors_again_after_a_reload() {
    let root = folder("paragraph");
    std::fs::write(
        root.join("a.md"),
        "# A\n\n- Writes\n  - retried   until acknowledged\n  - dropped after a day\n- Reads\n",
    )
    .unwrap();
    let (mut browser, page, events) = open("paragraph", &root, &root.join("a.md"), "/a.md").await;

    eval::<bool>(
        &page,
        "(document.querySelector('article li').dispatchEvent(new MouseEvent('mouseover', { bubbles: true })), true)",
    )
    .await;
    assert!(until(&page, "!!document.querySelector('.comment-button')").await);
    eval::<bool>(
        &page,
        "document.querySelector('.comment-button').click(), true",
    )
    .await;
    let quote: String = eval(
        &page,
        "document.querySelector('#devpage-comments .composer blockquote').textContent",
    )
    .await;
    assert_eq!(
        quote,
        "Writes retried until acknowledged dropped after a day"
    );
    eval::<bool>(
        &page,
        "(() => { const box = document.querySelector('#devpage-comments .composer textarea');
           box.value = 'Why a day?'; box.closest('form').requestSubmit(); return true; })()",
    )
    .await;
    assert!(
        until(
            &page,
            "document.querySelectorAll('mark.comment').length > 0"
        )
        .await
    );

    page.reload().await.unwrap();
    loaded(&page).await;
    assert!(
        until(
            &page,
            "[...document.querySelectorAll('mark.comment')].map(m => m.textContent).join('|')
               .replace(/\\s+/g, ' ') .includes('retried until acknowledged')
             && !(document.getElementById('devpage-comments')?.textContent ?? '').includes('Detached')"
        )
        .await
    );
    browser.close().await.ok();
    events.abort();
}

#[tokio::test]
#[ignore]
async fn on_a_deck_c_toggles_the_comments_panel() {
    let root = folder("deck");
    std::fs::write(
        root.join("devpage.yml"),
        "kind: slides\ntitle: Talk\nslides: [one.md, two.md]\n",
    )
    .unwrap();
    std::fs::write(root.join("one.md"), "# One\n\nThe opening line.\n").unwrap();
    std::fs::write(root.join("two.md"), "# Two\n").unwrap();
    let anchor = Anchor {
        quote: "opening line".into(),
        prefix: "The ".into(),
        suffix: ".".into(),
    };
    yaml::Yaml::new(&root)
        .create("/one.md", anchor, "Bo", "Sharper?")
        .unwrap();
    let (mut browser, page, events) =
        open("deck", &root, &root.join("devpage.yml"), "/devpage.yml").await;

    let shown = "getComputedStyle(document.getElementById('devpage-comments')).display !== 'none'";
    assert!(until(&page, "!!document.querySelector('mark.comment')").await);
    assert!(!eval::<bool>(&page, shown).await, "hidden until asked for");
    eval::<bool>(
        &page,
        "(document.dispatchEvent(new KeyboardEvent('keydown', { key: 'c' })), true)",
    )
    .await;
    assert!(until(&page, shown).await);
    let text: String = eval(
        &page,
        "document.getElementById('devpage-comments').innerText",
    )
    .await;
    assert!(text.contains("Sharper?"), "{text}");
    eval::<bool>(
        &page,
        "(document.dispatchEvent(new KeyboardEvent('keydown', { key: 'c' })), true)",
    )
    .await;
    assert!(until(&page, &format!("!({shown})")).await);
    browser.close().await.ok();
    events.abort();
}

#[tokio::test]
#[ignore]
async fn a_live_update_with_the_comments_shown_swaps_the_page_in_place() {
    let root = folder("shown");
    std::fs::write(root.join("a.md"), "# A\n\nThe first draft.\n").unwrap();
    let (mut browser, page, events) = open("shown", &root, &root.join("a.md"), "/a.md").await;

    // Showing the comments marks the body; a fetched page never carries
    // that mark, and it must not make the update a full reload.
    eval::<bool>(
        &page,
        "(document.body.classList.add('show-comments'), window.notReloaded = true, true)",
    )
    .await;
    std::fs::write(root.join("a.md"), "# A\n\nThe second draft.\n").unwrap();
    assert!(
        until(
            &page,
            "document.querySelector('article p')?.textContent === 'The second draft.'"
        )
        .await
    );
    assert!(
        eval::<bool>(&page, "window.notReloaded === true").await,
        "the page was swapped in place, not reloaded"
    );
    assert!(
        eval::<bool>(&page, "document.body.classList.contains('show-comments')").await,
        "the comments stay shown"
    );
    browser.close().await.ok();
    events.abort();
}
