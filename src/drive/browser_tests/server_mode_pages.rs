//! A published document's pages in chrome-headless-shell: a page open at the
//! link swaps in each new version, an old version says so and links to the
//! latest, and the version menu and the download button.

use std::time::Duration;

use super::{publish, server_mode, zip_of};
use crate::chrome::testing::{self, eval, until};

fn doc(text: &str) -> Vec<u8> {
    zip_of(&[
        (
            "devpage.yml",
            "kind: doc\ntitle: Rollout\nchapters:\n  - rollout.md\n",
        ),
        ("rollout.md", &format!("# Rollout\n\n{text}\n")),
    ])
}

/// `devpage server` on a free port, against a bucket in a folder named for
/// `name`: its address.
async fn start(name: &str) -> String {
    testing::serve(server_mode(name)).await
}

const TEXT: &str = "document.getElementById('devpage-page').textContent";
const MENU: &str = "document.querySelector('#devpage-versions summary')?.textContent";

#[tokio::test(flavor = "multi_thread")]
#[ignore = "drives Chrome"]
async fn a_page_open_at_the_link_swaps_in_each_new_version() {
    let base = start("live").await;
    let id = publish(&base, None, doc("The first plan.")).await;
    let browser = testing::launch().await;
    let page = testing::open(&browser, &format!("{base}/d/{id}/")).await;
    until(&page, &format!("{MENU} === 'Version 1 · latest'")).await;
    assert!(
        eval::<String>(&page, TEXT)
            .await
            .contains("The first plan.")
    );
    // The link leads to the doc's address, still under the document.
    let at: String = eval(&page, "location.pathname").await;
    assert_eq!(at, format!("/d/{id}/devpage.yml"));
    // Gone if the page is loaded again rather than swapped.
    eval::<bool>(&page, "window.sameLoad = true").await;

    publish(&base, Some(&id), doc("The second plan.")).await;
    until(&page, &format!("{TEXT}.includes('The second plan.')")).await;
    until(&page, &format!("{MENU} === 'Version 2 · latest'")).await;
    // Swapped in place, at the same address: the latest, not version 2's.
    assert_eq!(eval::<String>(&page, "location.pathname").await, at);
    assert!(
        eval::<bool>(&page, "window.sameLoad === true").await,
        "swapped, not reloaded"
    );
    // And the next publish reaches it too.
    publish(&base, Some(&id), doc("The third plan.")).await;
    until(&page, &format!("{TEXT}.includes('The third plan.')")).await;
    assert!(
        eval::<bool>(&page, "!document.getElementById('devpage-version-banner')").await,
        "the latest has no banner"
    );
    drop(browser);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "drives Chrome"]
async fn an_old_version_says_so_never_changes_and_lists_every_version() {
    let base = start("old").await;
    let id = publish(&base, None, doc("The first plan.")).await;
    publish(&base, Some(&id), doc("The second plan.")).await;
    let browser = testing::launch().await;
    let page = testing::open(&browser, &format!("{base}/d/{id}/v/1/")).await;
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
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "drives Chrome"]
async fn a_docs_download_button_saves_its_pdf_printed_once() {
    let base = start("pdf").await;
    let id = publish(&base, None, doc("The plan, on paper.")).await;
    let browser = testing::launch().await;
    let page = testing::open(&browser, &format!("{base}/d/{id}/")).await;
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

/// The old version's banner sits above the page, not over it, and the
/// version bar leaves the end of a long sidebar in sight.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "drives Chrome"]
async fn the_banner_and_the_version_bar_cover_nothing() {
    use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
    let base = start("cover").await;
    let id = publish(&base, None, doc("The first plan.")).await;
    publish(&base, Some(&id), doc("The second plan.")).await;
    let mut files = vec![(
        "devpage.yml".to_string(),
        "kind: site\ntitle: Long site\nroot: README.md\n".to_string(),
    )];
    let mut readme = "# Home\n\n## Topics\n\n".to_string();
    for i in 1..=40 {
        readme += &format!("- [Page {i}](p{i}.md) — page {i}\n");
        files.push((format!("p{i}.md"), format!("# Page {i}\n")));
    }
    files.push(("README.md".into(), readme));
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let site = publish(&base, None, zip_of(&files)).await;
    publish(&base, Some(&site), zip_of(&files)).await;
    let browser = testing::launch().await;
    const SHOWN: &str = "!!document.querySelector('#devpage-versions summary')
        && !!document.getElementById('devpage-version-banner')";

    // A doc, wide and narrow: the banner ends where the page begins.
    for (width, height) in [(1400, 900), (800, 600)] {
        let page = testing::open(&browser, &format!("{base}/d/{id}/v/1/")).await;
        page.execute(SetDeviceMetricsOverrideParams::new(
            width, height, 1.0, false,
        ))
        .await
        .unwrap();
        until(&page, SHOWN).await;
        let gap: f64 = eval(
            &page,
            "document.getElementById('devpage-page').getBoundingClientRect().top
             - document.getElementById('devpage-version-banner').getBoundingClientRect().bottom",
        )
        .await;
        assert!(
            gap >= 0.0,
            "at {width}px the banner covers the page by {}px",
            -gap
        );
        page.close().await.ok();
    }

    // A site with a long sidebar, scrolled to its end: its last link is
    // above the version bar.
    let page = testing::open(&browser, &format!("{base}/d/{site}/v/1/")).await;
    page.execute(SetDeviceMetricsOverrideParams::new(1400, 700, 1.0, false))
        .await
        .unwrap();
    until(&page, SHOWN).await;
    let gap: f64 = eval(
        &page,
        "(() => { const nav = document.getElementById('devpage-nav');
           nav.scrollTop = nav.scrollHeight;
           const links = nav.querySelectorAll('a');
           return document.getElementById('devpage-versions').getBoundingClientRect().top
             - links[links.length - 1].getBoundingClientRect().bottom; })()",
    )
    .await;
    assert!(
        gap >= 0.0,
        "the version bar covers the sidebar's last link by {}px",
        -gap
    );
    page.close().await.ok();
    drop(browser);
}
