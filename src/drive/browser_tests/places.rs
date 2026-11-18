//! A document's places and the page module, in chrome-headless-shell: widgets
//! shown outside the text on every page, and a widget reading and steering
//! the page it is on.

use std::path::Path;
use std::sync::Arc;

use chromiumoxide::Page;

use super::page_script::{rewrite, text, widget_folder};
use crate::adapters::chrome::testing::{self, eval, until, until_is};
use crate::drive::http;
use devpage_core::document::Document;
use devpage_core::folder::ServedFolder;

/// Serves `dir` with `docs` as the entry and opens `path` in a headless
/// Chrome `width` wide, once every widget in the text has mounted.
async fn open(dir: &Path, path: &str, width: u32) -> (testing::Browser, Page) {
    let document =
        Arc::new(Document::load(&dir.join("docs"), &ServedFolder::new(dir).unwrap()).unwrap());
    let changes = crate::adapters::changes::Changes::default();
    let address = testing::serve(http::router(
        http::App::testing(document, changes)
            .with_comments_on_disk(devpage_core::comments::Identity::Local("T".into())),
    ))
    .await;
    let browser = testing::browser_on(width, 900).await;
    let page = testing::open(&browser, &format!("{address}{path}")).await;
    (browser, page)
}

/// A site with a navbar and a widget in each place; `Dot` counts its mounts.
fn placed_folder(name: &str) -> std::path::PathBuf {
    widget_folder(
        name,
        "# Home\n\nTo the [other](other.md) page.\n",
        &[
            (
                "docs/devpage.yml",
                "kind: site\ntitle: W\nroot: README.md\nwidgets: widgets\nnavbar:\n  Home: README.md\nplaces:\n  navbar: [Dot]\n  panel: [Side]\n  float: [Bar]\n",
            ),
            ("docs/other.md", "# Other\n\nAnother page.\n"),
            (
                "docs/widgets/Dot.tsx",
                "import { useEffect } from \"react\";\nexport default () => { useEffect(() => { window.dots = (window.dots || 0) + 1; }, []); return <b className=\"dot\">one</b>; };",
            ),
            (
                "docs/widgets/Side.tsx",
                "export default () => <p className=\"notes\">side</p>;",
            ),
            (
                "docs/widgets/Bar.tsx",
                "export default () => <p className=\"bar\">bar</p>;",
            ),
        ],
    )
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_places_widgets_show_on_every_page_and_stay_mounted() {
    let dir = placed_folder("places");
    let (mut browser, page) = open(&dir, "/docs/README.md", 1400).await;
    until(&page, "!!document.querySelector('#devpage-navbar .navbar-end > #devpage-place-navbar:first-child .dot')").await;
    until(
        &page,
        "!!document.querySelector('#devpage-place-panel .notes')",
    )
    .await;
    until(
        &page,
        "!!document.querySelector('#devpage-place-float .bar')",
    )
    .await;
    // On a wide window the panel is beside the page, which gives it the room.
    assert_eq!(
        text(&page, "[document.body.className.includes('panel-beside'), getComputedStyle(document.getElementById('devpage-place-panel')).display, getComputedStyle(document.body).paddingRight].join(' ')").await,
        "true block 300px"
    );
    // A widget closes and opens it.
    assert_eq!(
        text(&page, "(window.devpagePage.page.panel(false), getComputedStyle(document.getElementById('devpage-place-panel')).display + ' ' + getComputedStyle(document.body).paddingRight)").await,
        "none 0px"
    );
    eval::<bool>(&page, "(window.devpagePage.page.panel(null), true)").await;

    // Another page swapped in: the same widgets, never mounted again, the
    // navbar's back in the fresh navbar.
    assert_eq!(
        text(&page, "load('/docs/other.md', 'push').then(String)").await,
        "true"
    );
    assert_eq!(
        text(&page, "document.querySelector('article h1').textContent").await,
        "Other"
    );
    eval::<bool>(&page, "placing.then(() => true)").await;
    assert_eq!(
        text(&page, "[!!document.querySelector('#devpage-navbar .navbar-end > #devpage-place-navbar .dot'), !!document.querySelector('#devpage-place-panel .notes'), window.dots].join(' ')").await,
        "true true 1"
    );

    // An edited widget is mounted again; the others are left be.
    rewrite(
        &dir.join("docs/widgets/Dot.tsx"),
        "export default () => <b className=\"dot\">two</b>;",
    );
    page.evaluate("load(location.href)").await.unwrap();
    until_is(
        &page,
        "document.querySelector('#devpage-place-navbar .dot')?.textContent ?? ''",
        "two",
    )
    .await;
    assert_eq!(
        text(
            &page,
            "String(document.querySelectorAll('#devpage-place-navbar .dot').length)"
        )
        .await,
        "1"
    );
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn on_a_narrow_window_the_panel_opens_only_when_a_widget_opens_it() {
    let dir = placed_folder("places-narrow");
    let (mut browser, page) = open(&dir, "/docs/README.md", 700).await;
    until(
        &page,
        "!!document.querySelector('#devpage-place-panel .notes')",
    )
    .await;
    let shown = "getComputedStyle(document.getElementById('devpage-place-panel')).display + ' ' + getComputedStyle(document.body).paddingRight";
    assert_eq!(text(&page, shown).await, "none 0px");
    eval::<bool>(&page, "(window.devpagePage.page.panel(true), true)").await;
    assert_eq!(
        text(&page, shown).await,
        "block 0px",
        "over the page, not beside it"
    );
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_widget_reads_and_steers_the_page_through_the_page_module() {
    let dir = widget_folder(
        "page-module",
        "# Home\n\nThe first paragraph has words.\n\nTo the [other](other.md) page.\n\n```mermaid\nflowchart LR\n  A[Sowing] --> B[Harvest]\n```\n\n<Probe />\n",
        &[
            ("docs/other.md", "# Other\n\nAnother page with words.\n"),
            (
                "docs/widgets/Probe.tsx",
                "import { page, base } from \"devpage\";\nwindow.probe = { page, base };\nexport default () => <i className=\"probe\" />;",
            ),
        ],
    );
    let (mut browser, page) = open(&dir, "/docs/README.md", 1400).await;
    until(&page, "!!window.probe").await;
    assert_eq!(
        text(&page, "JSON.stringify(probe.page.current())").await,
        r#"{"address":"/docs/README.md","fragment":"","sources":["/docs/README.md"]}"#
    );
    assert!(
        text(&page, "probe.page.text()")
            .await
            .contains("The first paragraph has words.")
    );
    assert_eq!(
        text(&page, "String(probe.page.text('/docs/gone.md'))").await,
        "null"
    );
    assert_eq!(
        text(
            &page,
            "(() => { try { probe.base('none'); } catch (error) { return error.message; } })()"
        )
        .await,
        "devpage: the document has no extension named none"
    );

    // A part is found again, marked under a name, and unmarked.
    let part = "{ page: '/docs/README.md', quote: 'first paragraph' }";
    assert_eq!(
        text(&page, &format!("probe.page.range({part}).toString()")).await,
        "first paragraph"
    );
    assert_eq!(
        text(
            &page,
            "String(probe.page.range({ page: '/docs/README.md', quote: 'not there' }))"
        )
        .await,
        "null"
    );
    assert_eq!(
        text(&page, &format!("(probe.page.mark('here', [{part}]), document.querySelector('mark.devpage-mark[data-mark=\"here\"]').textContent)")).await,
        "first paragraph"
    );
    assert_eq!(
        text(&page, "(probe.page.mark('here'), String(document.querySelectorAll('mark.devpage-mark').length) + ' ' + document.querySelector('article p').childNodes.length)").await,
        "0 1"
    );

    // A diagram's text is marked where it is, and stays drawn.
    assert_eq!(
        text(&page, "(probe.page.mark('here', [{ page: '/docs/README.md', quote: 'Sowing' }]), (() => { const mark = document.querySelector('.diagram tspan.devpage-mark'); return mark.textContent + ' ' + (mark.getBoundingClientRect().width > 0) + ' ' + document.querySelectorAll('.diagram mark').length; })())").await,
        "Sowing true 0"
    );
    assert_eq!(
        text(
            &page,
            "(probe.page.mark('here'), String(document.querySelectorAll('.devpage-mark').length))"
        )
        .await,
        "0"
    );

    // The selection, as a part, read and told.
    eval::<bool>(
        &page,
        "(window.told = [], probe.page.on('select', (part) => told.push(part)), true)",
    )
    .await;
    eval::<bool>(
        &page,
        "(() => { const words = document.querySelector('article p').firstChild; const range = document.createRange();
           range.setStart(words, 4); range.setEnd(words, 9); getSelection().removeAllRanges(); getSelection().addRange(range); return true; })()",
    )
    .await;
    assert_eq!(
        text(&page, "JSON.stringify(probe.page.selection())").await,
        r#"{"page":"/docs/README.md","quote":"first","prefix":"Home The ","suffix":" paragraph has words. To the oth"}"#
    );
    until(&page, "told.length === 1 && told[0].quote === 'first'").await;
    eval::<bool>(&page, "(getSelection().removeAllRanges(), true)").await;
    until(&page, "told.length === 2 && told[1] === null").await;

    // Picking: a click on a block's text is a pick, a click beside the text
    // picks nothing, and a link is still a link.
    eval::<bool>(&page, "(window.picked = [], window.stop = probe.page.on('pick', (part) => picked.push(part)), probe.page.pick(true), true)").await;
    eval::<bool>(&page, "(document.querySelector('article p a').dispatchEvent(new MouseEvent('mouseover', { bubbles: true })), true)").await;
    assert_eq!(
        text(&page, "document.querySelector('.devpage-pick').tagName").await,
        "P"
    );
    eval::<bool>(
        &page,
        "(document.querySelectorAll('article p')[1].click(), document.querySelector('#devpage-page').click(), true)",
    )
    .await;
    assert_eq!(
        text(
            &page,
            "picked.map((part) => part ? part.page + ' ' + part.quote : 'nothing').join('|') + ' ' + location.pathname"
        )
        .await,
        "/docs/README.md To the other page.|nothing /docs/README.md"
    );
    eval::<bool>(
        &page,
        "(document.querySelector('article p a').click(), true)",
    )
    .await;
    until(&page, "location.pathname === '/docs/other.md'").await;
    assert_eq!(text(&page, "String(picked.length)").await, "2");
    eval::<bool>(&page, "probe.page.go('/docs/README.md').then(() => true)").await;
    eval::<bool>(&page, "(probe.page.pick(false), stop(), true)").await;
    assert_eq!(
        text(
            &page,
            "String(document.querySelectorAll('.devpage-pick').length)"
        )
        .await,
        "0"
    );

    // Going to a page tells the change; what is marked shows where it is.
    eval::<bool>(&page, "(window.changes = [], probe.page.on('change', (now) => changes.push(now.address)), probe.page.mark('there', [{ page: '/docs/other.md', quote: 'Another page' }]), true)").await;
    eval::<bool>(&page, "probe.page.go('/docs/other.md').then(() => true)").await;
    assert_eq!(
        text(&page, "[document.querySelector('article h1').textContent, changes.join(), document.querySelector('mark.devpage-mark')?.textContent, location.pathname].join(' | ')").await,
        "Other | /docs/other.md | Another page | /docs/other.md"
    );
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}
