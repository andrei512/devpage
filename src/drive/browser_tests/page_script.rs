//! The page script in chrome-headless-shell: widgets mounting and live
//! updates, decks, the outline, the navbar, the theme, diagrams, the sidebar
//! and its search, a reload's caching, swaps between pages and the comment
//! marks inside widgets.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chromiumoxide::Page;
use futures::StreamExt;

use crate::chrome::testing::{self, Answered, eval, until};
use crate::document::{Document, Kind};
use crate::folder::ServedFolder;
use crate::server;

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn widgets_mount_on_load_and_after_a_live_update() {
    let dir = std::env::temp_dir().join(format!("devpage-widgets-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("docs/widgets")).unwrap();
    let dir = dir.canonicalize().unwrap();
    std::fs::write(
        dir.join("docs/devpage.yml"),
        "kind: site\ntitle: W\nroot: README.md\nwidgets: widgets\n",
    )
    .unwrap();
    std::fs::write(
            dir.join("docs/widgets/Pair.tsx"),
            "import { useState } from \"react\";\nexport default function Pair() { const [n] = useState(2); return <div className=\"pair\">{[...Array(n)].map((_, i) => <Callout title={String(i)}>x</Callout>)}</div>; }\n",
        )
        .unwrap();
    let page_file = dir.join("docs/README.md");
    std::fs::write(
        &page_file,
        "# W\n\n<Callout kind=\"warning\">\nSome **markdown**\n</Callout>\n\n<Pair />\n",
    )
    .unwrap();
    let document =
        Arc::new(Document::load(&dir.join("docs"), &ServedFolder::new(&dir).unwrap()).unwrap());
    let changes = crate::changes::Changes::default();
    let router = server::router(
        server::App::testing(document, changes)
            .with_comments_on_disk(crate::comments::Identity::Local("T".into())),
    );
    let address = testing::serve(router).await;
    let mut browser = testing::launch().await;
    let page = browser
        .new_page(format!("{address}/docs/README.md").as_str())
        .await
        .unwrap();
    let count = |page: Page| async move {
        let value: String = page
                .evaluate("document.documentElement.dataset.ready + ':' + document.querySelectorAll('aside.callout').length + ':' + !!document.querySelector('.callout-warning strong')")
                .await
                .unwrap()
                .into_value()
                .unwrap();
        value
    };
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(count(page.clone()).await, "true:3:true");
    std::fs::write(
        &page_file,
        "# W\n\n<Callout>one</Callout>\n\n<Callout>two</Callout>\n\n<Pair />\n",
    )
    .unwrap();
    page.evaluate("load(location.href)").await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(count(page.clone()).await, "true:4:false");
    // An edited widget is fetched again on the next live update.
    std::fs::write(
            dir.join("docs/widgets/Pair.tsx"),
            "export default function Pair() { return <div className=\"pair\"><Callout>only</Callout></div>; }\n",
        )
        .unwrap();
    page.evaluate("load(location.href)").await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(count(page.clone()).await, "true:3:false");
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_deck_shows_one_slide_and_moves_with_the_arrow_keys() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let document = Arc::new(
        Document::load(
            &root.join("docs/talks/devpage"),
            &ServedFolder::new(root).unwrap(),
        )
        .unwrap(),
    );
    let Kind::Slides { slides, .. } = &document.kind else {
        panic!()
    };
    let last = format!("{0}#{0}", slides.len());
    let url = document.entry();
    let changes = crate::changes::Changes::default();
    let router = server::router(
        server::App::testing(document, changes)
            .with_comments_on_disk(crate::comments::Identity::Local("T".into())),
    );
    let address = testing::serve(router).await;
    let mut browser = testing::launch().await;
    let page = browser
        .new_page(format!("{address}{url}").as_str())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let shown = |page: Page| async move {
        let value: String = page
                .evaluate("[...document.querySelectorAll('.slide.shown')].map(s => s.dataset.slide).join(',') + location.hash")
                .await
                .unwrap()
                .into_value()
                .unwrap();
        value
    };
    assert_eq!(shown(page.clone()).await, "1");
    page.evaluate("document.dispatchEvent(new KeyboardEvent('keydown', {key: 'ArrowRight'}))")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(shown(page.clone()).await, "2#2");
    page.evaluate("document.dispatchEvent(new KeyboardEvent('keydown', {key: 'End'}))")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(shown(page.clone()).await, last);
    // ⌘← is the browser's, not the deck's.
    page.evaluate(
        "document.dispatchEvent(new KeyboardEvent('keydown', {key: 'ArrowLeft', metaKey: true}))",
    )
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(shown(page.clone()).await, last);
    // A live update swaps the deck in place and stays on the slide shown.
    page.evaluate("load(location.href)").await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(shown(page.clone()).await, last);
    browser.close().await.ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_deck_served_with_comments_off_shows_no_comment_ui() {
    let dir = widget_folder(
        "no-comments",
        "# W\n",
        &[
            (
                "docs/talk.yml",
                "kind: slides\ntitle: T\nslides: [one.md]\n",
            ),
            ("docs/one.md", "# One\n\nThe proxy names the reader.\n"),
            (
                ".devpage/comments/docs/one.md/20261025T000000Z-0001.yml",
                "anchor:\n  quote: names the reader\nthread:\n- author: T\n  at: 2026-10-25T00:00:00Z\n  text: which header?\n",
            ),
        ],
    );
    let document = Arc::new(
        Document::load(
            &dir.join("docs/talk.yml"),
            &ServedFolder::new(&dir).unwrap(),
        )
        .unwrap(),
    );
    let changes = crate::changes::Changes::default();
    let router = server::router(server::App::testing(document, changes));
    let address = testing::serve(router).await;
    let mut browser = testing::launch().await;
    let page = browser
        .new_page(format!("{address}/docs/talk.yml").as_str())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    // Even with comment mode remembered as on, nothing offers a comment.
    page.evaluate("localStorage.setItem('devpage-comment-mode', 'on')")
        .await
        .unwrap();
    page.evaluate("document.querySelector('.slide.shown p').dispatchEvent(new MouseEvent('mouseover', {bubbles: true}))")
            .await
            .unwrap();
    page.evaluate("document.dispatchEvent(new KeyboardEvent('keydown', {key: 'c'}))")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
            text(
                &page,
                "document.documentElement.dataset.ready + ':' + ['.comment-mode', '#devpage-comments-toggle', '#devpage-comments', 'mark.comment', '.comment-button'].filter(s => document.querySelector(s)).join(',') + ':' + document.body.classList.contains('show-comments')"
            )
            .await,
            "true::false"
        );
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_docs_outline_moves_above_the_text_on_a_narrow_screen() {
    use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let document = Arc::new(
        Document::load(
            &root.join("tests/fixtures/sync-design"),
            &ServedFolder::new(root).unwrap(),
        )
        .unwrap(),
    );
    let url = document.entry();
    let changes = crate::changes::Changes::default();
    let router = server::router(
        server::App::testing(document, changes)
            .with_comments_on_disk(crate::comments::Identity::Local("T".into())),
    );
    let address = testing::serve(router).await;
    let mut browser = testing::launch().await;
    let page = browser.new_page("about:blank").await.unwrap();
    let layout = |width: i64| {
        let page = page.clone();
        let url = format!("{address}{url}");
        async move {
            page.execute(SetDeviceMetricsOverrideParams::new(width, 900, 1.0, false))
                .await
                .unwrap();
            page.goto(url.as_str()).await.unwrap();
            tokio::time::sleep(Duration::from_millis(300)).await;
            let value: String = page
                    .evaluate("(() => { const n = document.getElementById('devpage-nav').getBoundingClientRect(), a = document.querySelector('article').getBoundingClientRect(); return n.bottom <= a.top ? 'above' : n.right <= a.left ? 'beside' : 'overlap'; })()")
                    .await
                    .unwrap()
                    .into_value()
                    .unwrap();
            value
        }
    };
    assert_eq!(layout(1300).await, "beside");
    assert_eq!(layout(600).await, "above");
    browser.close().await.ok();
}

/// A served folder for the widget tests: `docs/` is a site with the given
/// README, and `files` are written beside it.
pub(super) fn widget_folder(name: &str, readme: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("devpage-w-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("docs/widgets")).unwrap();
    let dir = dir.canonicalize().unwrap();
    std::fs::write(
        dir.join("docs/devpage.yml"),
        "kind: site\ntitle: W\nroot: README.md\nwidgets: widgets\n",
    )
    .unwrap();
    std::fs::write(dir.join("docs/README.md"), readme).unwrap();
    for (file, text) in files {
        let path = dir.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    dir
}

/// Serves `dir` with `docs` as the entry and opens `path` in headless
/// Chrome, once every widget has mounted.
pub(super) async fn open(dir: &Path, path: &str) -> (testing::Browser, Page) {
    let (browser, page, _) = open_logged(dir, path).await;
    (browser, page)
}

/// [`open`], also keeping every request the server answers.
async fn open_logged(dir: &Path, path: &str) -> (testing::Browser, Page, Answered) {
    let document =
        Arc::new(Document::load(&dir.join("docs"), &ServedFolder::new(dir).unwrap()).unwrap());
    let changes = crate::changes::Changes::default();
    let (router, answered) = testing::answering(server::router(
        server::App::testing(document, changes)
            .with_comments_on_disk(crate::comments::Identity::Local("T".into())),
    ));
    let address = testing::serve(router).await;
    let browser = testing::launch().await;
    let page = testing::open(&browser, &format!("{address}{path}")).await;
    (browser, page, answered)
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_cold_load_asks_for_every_module_from_the_head_and_a_plain_page_for_its_sidebar_only() {
    use chromiumoxide::cdp::browser_protocol::network::{
        EnableParams, EventRequestWillBeSent, InitiatorType, ResourceType,
    };
    let dir = widget_folder(
        "one-round-trip",
        "# Home\n\n<Card />\n\n```mermaid\nflowchart LR\n  A --> B\n```\n\n![Checkout](checkout.design)\n",
        &[
            (
                "docs/widgets/Card.tsx",
                "import { label } from \"./label\";\nexport default function Card() { return <Tabs><Tab title={label}>x</Tab></Tabs>; }\n",
            ),
            ("docs/widgets/label.ts", "export const label = 'l';\n"),
            ("docs/plain.md", "# Plain\n\nJust text.\n"),
            ("docs/checkout.design", ""),
        ],
    );
    // A plain page fetches no widget but its sidebar.
    let (mut browser, page, answered) = open_logged(&dir, "/docs/plain.md").await;
    assert_eq!(
        text(&page, "String(document.documentElement.dataset.ready)").await,
        "true"
    );
    let log = answered.lock().unwrap().clone();
    assert!(
        log.iter()
            .any(|(p, _)| p.starts_with("/__devpage/devpage.")),
        "{log:?}"
    );
    assert!(
        !log.iter().any(|(p, _)| p.contains("pele")
            || (p.contains("widgets") && !p.ends_with("/DevpageSidebar.tsx"))),
        "{log:?}"
    );
    assert!(
        log.iter()
            .any(|(p, _)| p.ends_with("/system-widgets/DevpageSidebar.tsx")),
        "{log:?}"
    );
    // What the plain page fetched — React and the sidebar — the browser
    // keeps: the next page doesn't download it again.
    let warm: Vec<String> = log.iter().map(|(p, _)| p.clone()).collect();
    let address = page.url().await.unwrap().unwrap();
    let address = address.split("/docs/").next().unwrap().to_string();

    // Cold — none of its own modules fetched yet: every module the page
    // imports was asked for by its head, not by a script.
    let cold = browser.new_page("about:blank").await.unwrap();
    cold.execute(EnableParams::default()).await.unwrap();
    let mut sent = cold
        .event_listener::<EventRequestWillBeSent>()
        .await
        .unwrap();
    answered.lock().unwrap().clear();
    cold.goto(format!("{address}/docs/README.md"))
        .await
        .unwrap();
    testing::loaded(&cold).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
            text(
                &cold,
                "String(!!document.querySelector('.diagram svg') && !!document.querySelector('[data-widget=Card] .tabs'))"
            )
            .await,
            "true",
            "the diagram and the card drew"
        );
    let mut modules = Vec::new();
    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(200), sent.next()).await
    {
        if event.r#type == Some(ResourceType::Script)
            && event.request.url.contains("/__devpage/")
            && !event.request.url.contains("/__devpage/devpage.")
        {
            modules.push((event.request.url.clone(), event.initiator.r#type.clone()));
        }
    }
    for name in [
        "react.",
        "Card.tsx",
        "label.ts",
        "Tabs.tsx",
        "Tab.tsx",
        "design.",
        "design-engine.",
        "DevpageSidebar.tsx",
    ] {
        assert!(
            modules.iter().any(|(url, _)| url.contains(name)),
            "{name} in {modules:?}"
        );
    }
    // The diagram came drawn: nothing was fetched to draw it.
    assert!(
        !modules
            .iter()
            .any(|(url, _)| url.contains("pele") || url.contains("Diagram")),
        "{modules:?}"
    );
    let by_script: Vec<_> = modules
        .iter()
        .filter(|(_, initiator)| *initiator == InitiatorType::Script)
        .collect();
    assert!(by_script.is_empty(), "started by script: {by_script:#?}");
    // Each downloaded once — or not at all, if the plain page did: the
    // preload and the import agree.
    let log = answered.lock().unwrap().clone();
    for (url, _) in &modules {
        let path = url[address.len()..].split('?').next().unwrap();
        assert_eq!(
            log.iter().filter(|(p, _)| p == path).count(),
            usize::from(!warm.iter().any(|p| p == path)),
            "{path} in {log:?}"
        );
    }

    // A page loaded in place brings its preloads into the head.
    assert_eq!(
            text(
                &page,
                "load('/docs/README.md').then(() => [...document.head.querySelectorAll('link[rel=modulepreload]')].some((l) => l.href.includes('/Card.tsx')) && !!document.querySelector('.diagram svg')).then(String)"
            )
            .await,
            "true"
        );
    browser.close().await.ok();
}

/// `script` evaluated in `page`, as a string.
pub(super) async fn text(page: &Page, script: &str) -> String {
    eval(page, script).await
}

const NAVBAR_SITE: &str = "kind: site\ntitle: Garden\nroot: README.md\nnavbar:\n  Home (house): README.md\n  Guide (book-open): guide.md\n  More (layout-grid):\n    Plan (file-text): plan.md\n    Web: https://example.com/garden\n  (github): https://github.com/someone/garden\n";

fn navbar_folder(name: &str) -> PathBuf {
    widget_folder(
        name,
        "# Home\n\n## Topics\n\n- [Guide](guide.md)\n- [Plan](plan.md)\n",
        &[
            ("docs/devpage.yml", NAVBAR_SITE),
            ("docs/guide.md", "# Guide\n\nRead on.\n"),
            ("docs/plan.md", "# Plan\n\nThe plan.\n"),
        ],
    )
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn the_logo_shows_before_the_title_and_is_the_tab_icon() {
    let logo = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 8 8\"><rect width=\"8\" height=\"8\" fill=\"#0066cc\"/></svg>";
    let dir = widget_folder(
        "navbar-logo",
        "# Home\n",
        &[
            (
                "docs/devpage.yml",
                &format!("{NAVBAR_SITE}logo: logo.svg\n"),
            ),
            ("docs/guide.md", "# Guide\n"),
            ("docs/plan.md", "# Plan\n"),
            ("docs/logo.svg", logo),
        ],
    );
    let (mut browser, page) = open(&dir, "/docs/README.md").await;
    // Loaded, 22px, before the title and centred on it, and the tab's icon.
    let shown = "(() => { const i = document.querySelector('.navbar-title .logo'), t = document.querySelector('.navbar-title'); const a = i.getBoundingClientRect(), b = t.getBoundingClientRect(); return [i.complete && i.naturalWidth > 0, a.width, a.height, i === t.firstElementChild, Math.abs((a.top + a.bottom) / 2 - (b.top + b.bottom) / 2) < 1.5, document.querySelector('link[rel=icon]').getAttribute('href')].join(' '); })()";
    assert_eq!(
        text(&page, shown).await,
        "true 22 22 true true /docs/logo.svg"
    );
    browser.close().await.ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn the_navbar_opens_its_dropdown_and_follows_the_page_shown() {
    let dir = navbar_folder("navbar-menu");
    let (mut browser, page) = open(&dir, "/docs/README.md").await;
    assert_eq!(
        text(
            &page,
            "document.querySelector('.navbar-link.current span').textContent"
        )
        .await,
        "Home"
    );
    // Click opens the dropdown; Escape closes it.
    page.evaluate("document.querySelector('.navbar-menu > button').click()")
        .await
        .unwrap();
    assert_eq!(
        text(
            &page,
            "getComputedStyle(document.querySelector('.navbar-dropdown')).display"
        )
        .await,
        "grid"
    );
    assert_eq!(
        text(
            &page,
            "document.querySelector('.navbar-menu > button').getAttribute('aria-expanded')"
        )
        .await,
        "true"
    );
    page.evaluate("document.querySelector('.navbar-menu > button').dispatchEvent(new KeyboardEvent('keydown', {key: 'Escape', bubbles: true}))").await.unwrap();
    assert_eq!(
        text(
            &page,
            "getComputedStyle(document.querySelector('.navbar-dropdown')).display"
        )
        .await,
        "none"
    );
    // ArrowDown opens it and focuses the first entry.
    page.evaluate("document.querySelector('.navbar-menu > button').dispatchEvent(new KeyboardEvent('keydown', {key: 'ArrowDown', bubbles: true}))").await.unwrap();
    assert_eq!(
        text(&page, "document.activeElement.textContent").await,
        "Plan"
    );
    // Following a dropdown link swaps the page in place and lights its menu.
    page.evaluate("document.activeElement.click()")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(text(&page, "location.pathname").await, "/docs/plan.md");
    assert_eq!(
        text(&page, "document.querySelector('.navbar-menu').className").await,
        "navbar-menu current"
    );
    assert_eq!(
        text(
            &page,
            "getComputedStyle(document.querySelector('.navbar-dropdown')).display"
        )
        .await,
        "none"
    );
    browser.close().await.ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn the_theme_toggle_switches_every_token_and_remembers_the_choice() {
    let dir = navbar_folder("navbar-theme");
    let (mut browser, page) = open(&dir, "/docs/README.md").await;
    let background = "getComputedStyle(document.body).backgroundColor";
    let theme = "document.documentElement.dataset.theme";
    // Shown icon: the moon in light, the sun in dark.
    let shown = "['to-dark','to-light'].filter(c => getComputedStyle(document.querySelector('.theme-toggle .' + c)).display !== 'none').join()";
    // Nothing saved: the page follows the system.
    assert_eq!(
        text(&page, theme).await,
        text(
            &page,
            "matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'"
        )
        .await
    );
    let start = text(&page, theme).await;
    let other = if start == "dark" { "light" } else { "dark" };
    page.evaluate("document.querySelector('.theme-toggle').click()")
        .await
        .unwrap();
    assert_eq!(text(&page, theme).await, other);
    assert_eq!(
        text(&page, "String(localStorage.getItem('devpage-theme'))").await,
        other
    );
    page.evaluate("document.querySelector('.theme-toggle').click()")
        .await
        .unwrap();
    page.evaluate("document.querySelector('.theme-toggle').click()")
        .await
        .unwrap();
    // Now dark wherever it started.
    if start == "dark" {
        page.evaluate("document.querySelector('.theme-toggle').click()")
            .await
            .unwrap();
    }
    assert_eq!(text(&page, theme).await, "dark");
    assert_eq!(text(&page, background).await, "rgb(0, 0, 0)");
    assert_eq!(text(&page, shown).await, "to-light");
    assert_eq!(
        text(
            &page,
            "document.querySelector('.theme-toggle').getAttribute('aria-label')"
        )
        .await,
        "Switch to light"
    );
    // A reload keeps it, before anything paints.
    page.reload().await.unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(text(&page, theme).await, "dark");
    assert_eq!(text(&page, background).await, "rgb(0, 0, 0)");
    // One click back to light, the moon showing.
    page.evaluate("document.querySelector('.theme-toggle').click()")
        .await
        .unwrap();
    assert_eq!(text(&page, background).await, "rgb(255, 255, 255)");
    assert_eq!(text(&page, shown).await, "to-dark");
    assert_eq!(
        text(&page, "String(localStorage.getItem('devpage-theme'))").await,
        "light"
    );
    browser.close().await.ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_diagram_comes_drawn_follows_the_theme_and_its_nodes_are_links() {
    let dir = widget_folder(
        "diagram-theme",
        "# Home\n\n```mermaid\nflowchart LR\n  a[Alpha] --> b[Beta]\n  click b href \"beta.md\" \"Beta\"\n```\n",
        &[("docs/beta.md", "# Beta page\n")],
    );
    let (mut browser, page) = open(&dir, "/docs/README.md").await;
    // In the HTML as served: nothing in the browser drew it.
    assert_eq!(
            text(
                &page,
                "fetch(location.href).then(r => r.text()).then(t => String(t.includes('<div class=\"diagram\"><svg')))"
            )
            .await,
            "true"
        );
    let fill = "getComputedStyle(document.querySelector('.diagram rect')).fill";
    let surface = "(() => { const probe = document.createElement('i'); probe.style.color = 'var(--surface)'; document.body.append(probe); const color = getComputedStyle(probe).color; probe.remove(); return color; })()";
    page.evaluate("document.documentElement.dataset.theme = 'light'")
        .await
        .unwrap();
    let light = text(&page, fill).await;
    assert_eq!(light, text(&page, surface).await);
    page.evaluate("document.querySelector('.theme-toggle').click()")
        .await
        .unwrap();
    assert_eq!(
        text(&page, "document.documentElement.dataset.theme").await,
        "dark"
    );
    let dark = text(&page, fill).await;
    assert_eq!(dark, text(&page, surface).await);
    assert_ne!(light, dark, "the diagram recolours with the theme");
    // A node's link opens its page in place.
    page.evaluate("document.querySelector('.diagram a[href=\"beta.md\"]').dispatchEvent(new MouseEvent('click', { bubbles: true, button: 0 }))")
            .await
            .unwrap();
    until(&page, "location.pathname === '/docs/beta.md' && document.querySelector('h1').textContent === 'Beta page'").await;
    browser.close().await.ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn on_a_narrow_screen_the_navbar_folds_into_a_menu() {
    use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
    let dir = navbar_folder("navbar-narrow");
    let (mut browser, page) = open(&dir, "/docs/README.md").await;
    page.execute(SetDeviceMetricsOverrideParams::new(390, 800, 1.0, false))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let fits = "(() => { const t = document.querySelector('.navbar-toggle').getBoundingClientRect(), s = document.querySelector('.theme-toggle').getBoundingClientRect(); return [t.right <= innerWidth, s.right <= innerWidth, document.documentElement.scrollWidth <= innerWidth].join(' '); })()";
    assert_eq!(text(&page, fits).await, "true true true");
    assert_eq!(
        text(
            &page,
            "getComputedStyle(document.querySelector('.navbar-links')).display"
        )
        .await,
        "none"
    );
    page.evaluate("document.querySelector('.navbar-toggle').click()")
        .await
        .unwrap();
    assert_eq!(
        text(
            &page,
            "getComputedStyle(document.querySelector('.navbar-links')).display"
        )
        .await,
        "flex"
    );
    browser.close().await.ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn every_system_widget_renders() {
    let long = "word ".repeat(80);
    let readme = format!(
        "{}\n\n<Example>\n{long}\n</Example>\n\n<Terminal>\n\n```\n$ {long}\n```\n\n</Terminal>\n\n<FileTree>\n\n- notes.md — kept as `code` and *emphasis*\n\n</FileTree>\n\n- [ ] open task\n- [x] done task\n\n<Keys>⌘ K</Keys> starts a line and **stays bold**.\n",
        "# W\n\n<Callout kind=\"tip\" title=\"Heads up\">\nSome **bold**\n</Callout>\n\n<Tabs>\n<Tab title=\"One\">\n\nfirst tab\n\n</Tab>\n<Tab title=\"Two\">\n\nsecond tab\n\n</Tab>\n</Tabs>\n\n<Example>\nA *word*\n</Example>\n\n<FileTree>\n\n- garden/\n  - [devpage.yml](README.md) — the site\n  - README.md\n\n</FileTree>\n\n<Terminal>\n\n```\n$ devpage check docs\nall good\n```\n\n</Terminal>\n\nPress <Keys>⌘ P</Keys> to print.\n\n```mermaid\nflowchart LR\n  A --> B\n```\n\n<Steps>\n\n1. Install it\n   From crates.io, then press <Keys>⌘ C</Keys>.\n\n   $ cargo install devpage\n\n2. Run it\n   $ devpage docs\n\n</Steps>\n\n<Cards columns={2}>\n\n- (rocket) [Start](README.md) — begin *here*\n- [Plain](README.md) — no icon\n- [Keys](README.md) — press <Keys>K</Keys>\n\n</Cards>\n\n<Styled />\n"
    );
    let dir = widget_folder(
        "system",
        &readme,
        &[(
            "docs/widgets/Styled.tsx",
            "export const css = `.styled { display: grid; }`;\nexport default function Styled() { return <div className=\"styled\">styled</div>; }\n",
        )],
    );
    let (mut browser, page) = open(&dir, "/docs/README.md").await;
    let report = text(
            &page,
            r#"JSON.stringify({
                callout: document.querySelector('aside.callout-tip strong')?.textContent + '|' + !!document.querySelector('aside.callout-tip .callout-body strong'),
                tabs: [...document.querySelectorAll('.tabs-bar button')].map(b => b.textContent).join(',') + '|' + document.querySelector('.tabs-panel').textContent.trim(),
                example: document.querySelector('.example-source').textContent.trim() + '|' + !!document.querySelector('.example-result em'),
                tree: [...document.querySelectorAll('.filetree')[0].querySelectorAll('li')].map(li => li.className).join(',') + '|' + document.querySelector('.filetree a')?.getAttribute('href') + '|' + document.querySelector('.filetree-note')?.textContent,
                terminal: document.querySelector('.terminal-command')?.textContent + '|' + document.querySelector('.terminal-output')?.textContent,
                keys: document.querySelectorAll('.keys kbd').length,
                diagram: !!document.querySelector('.diagram svg'),
                errors: document.querySelectorAll('.widget-error').length,
                highlighted: document.querySelectorAll('.example-source [class^="hl-"], .example-source [class*=" hl-"]').length > 0,
                scrolls: [...document.querySelectorAll('.example-source, .terminal pre')].map(p => p.scrollWidth > p.clientWidth && p.clientWidth > 0).join(','),
                pageFits: document.documentElement.scrollWidth <= innerWidth || document.documentElement.scrollWidth + ':' + innerWidth + ':' + [...document.querySelectorAll('#devpage-page *')].filter(e => e.getBoundingClientRect().right > innerWidth + 1 && !e.closest('.example-source, .terminal pre')).slice(0, 5).map(e => e.tagName + '.' + e.className).join(' '),
                note: [...document.querySelectorAll('.filetree-note')].map(n => n.innerHTML).find(h => h.includes('code')) ?? '',
                tasks: [...document.querySelectorAll('li')].filter(li => li.querySelector('input[type=checkbox]')).map(li => getComputedStyle(li).listStyleType).join(','),
                steps: [...document.querySelectorAll('.step')].map(li => li.querySelector('.step-number').textContent + ':' + li.querySelector('.step-title').textContent + ':' + [...li.querySelectorAll('.step-command')].map(c => c.textContent).join('/')).join(','),
                cards: [...document.querySelectorAll('.cards .card')].slice(0, 2).map(c => c.querySelector('.card-title').textContent + ':' + c.getAttribute('href') + ':' + !!c.querySelector('.card-icon svg') + ':' + (c.querySelector('.card-summary')?.innerHTML ?? '')).join(',') + '|' + getComputedStyle(document.querySelector('.cards')).gridTemplateColumns.split(' ').length,
                styled: getComputedStyle(document.querySelector('.styled')).display + '|' + (document.querySelector('style[data-widget=Styled]')?.textContent ?? '').trim().slice(0, 7),
                inner: document.querySelectorAll('.step .keys kbd').length + '|' + document.querySelectorAll('.cards .keys kbd').length + '|' + [...document.querySelectorAll('.steps, .cards')].some(e => e.textContent.includes('<Keys') || e.querySelector('[data-widget]:not([data-mounted])')),
                keysLine: [...document.querySelectorAll('p .keys')].filter(k => k.closest('p').textContent.includes('starts a line')).length + '|' + [...document.querySelectorAll('p strong')].some(s => s.textContent === 'stays bold'),
            })"#,
        )
        .await;
    let v: serde_json::Value = serde_json::from_str(&report).unwrap();
    assert_eq!(v["callout"], "Heads up|true", "{report}");
    assert_eq!(v["tabs"], "One,Two|first tab", "{report}");
    assert_eq!(v["example"], "A *word*|true", "{report}");
    assert_eq!(v["tree"], "folder,file,file|README.md|the site", "{report}");
    assert_eq!(v["terminal"], "$ devpage check docs|all good", "{report}");
    assert_eq!(v["keys"], 7, "{report}");
    assert_eq!(v["inner"], "2|1|false", "{report}");
    assert_eq!(v["styled"], "grid|.styled", "{report}");
    assert_eq!(v["diagram"], true, "{report}");
    assert_eq!(v["errors"], 0, "{report}");
    assert_eq!(v["highlighted"], true, "{report}");
    assert_eq!(v["scrolls"], "false,false,true,true", "{report}");
    assert_eq!(v["pageFits"], true, "{report}");
    assert_eq!(
        v["note"], "kept as <code>code</code> and <em>emphasis</em>",
        "{report}"
    );
    assert_eq!(v["tasks"], "none,none", "{report}");
    assert_eq!(v["keysLine"], "1|true", "{report}");
    assert_eq!(
        v["steps"], "1:Install it:$cargo install devpage,2:Run it:$devpage docs",
        "{report}"
    );
    assert_eq!(
        v["cards"], "Start→:README.md:true:begin <em>here</em>,Plain→:README.md:false:no icon|2",
        "{report}"
    );
    // On a wide window, where Example sets source and result side by side,
    // long lines still scroll inside their boxes rather than widen the page.
    page.execute(
        chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams::builder()
            .width(1400)
            .height(900)
            .device_scale_factor(1.0)
            .mobile(false)
            .build()
            .unwrap(),
    )
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
            text(&page, "[...document.querySelectorAll('.example-source, .terminal pre')].map(p => p.scrollWidth > p.clientWidth && getComputedStyle(p).overflowX === 'auto').join(',') + '|' + (document.documentElement.scrollWidth <= innerWidth) + '|' + [...document.querySelectorAll('.example')].every(e => e.getBoundingClientRect().right <= document.querySelector('#devpage-page').getBoundingClientRect().right + 1)").await,
            "false,false,true,true|true|true"
        );
    // Clicking the second tab shows it.
    page.evaluate("document.querySelectorAll('.tabs-bar button')[1].click()")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        text(
            &page,
            "document.querySelector('.tabs-panel').textContent.trim()"
        )
        .await,
        "second tab"
    );
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn widgets_work_in_a_doc_and_a_deck_and_a_page_loads_only_what_it_uses() {
    let dir = widget_folder(
        "kinds",
        "# W\n\n<Callout>home</Callout>\n",
        &[
            (
                "docs/widgets/Unused.tsx",
                "export default () => <b>never</b>;",
            ),
            (
                "docs/widgets/Badge.tsx",
                "export default ({ n }) => <span className=\"badge\">{n}</span>;",
            ),
            (
                "docs/widgets/Pair.tsx",
                "export default () => <div className=\"pair\"><Badge n={1} /><Badge n={2} /></div>;",
            ),
            (
                "docs/design/devpage.yml",
                "kind: doc\ntitle: D\nchapters: [one.md, two.md]\nwidgets: ../widgets\n",
            ),
            ("docs/design/one.md", "# One\n\n<Pair />\n"),
            (
                "docs/design/two.md",
                "# Two\n\n<Callout kind=\"danger\">careful</Callout>\n",
            ),
            (
                "docs/talk/devpage.yml",
                "kind: slides\ntitle: T\nslides: [a.md]\n",
            ),
            ("docs/talk/a.md", "# A\n\nPress <Keys>→</Keys>\n"),
        ],
    );
    let (mut browser, page) = open(&dir, "/docs/design/devpage.yml").await;
    assert_eq!(
            text(&page, "document.querySelectorAll('.pair .badge').length + ':' + document.querySelectorAll('aside.callout-danger').length").await,
            "2:1"
        );
    // Only the widgets the page names, and what they render, were fetched.
    let fetched = text(&page, "performance.getEntriesByType('resource').map(r => r.name).filter(n => n.includes('/__devpage/widgets/')).map(n => n.split('/').pop().split('?')[0]).sort().join(',')").await;
    assert_eq!(fetched, "Badge.tsx,Pair.tsx", "{fetched}");
    page.goto(
        page.url()
            .await
            .unwrap()
            .unwrap()
            .replace("/docs/design/devpage.yml", "/docs/talk/devpage.yml"),
    )
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(
        text(
            &page,
            "String(document.querySelectorAll('.slide .keys kbd').length)"
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
async fn an_edited_helper_reaches_open_pages() {
    let dir = widget_folder(
        "helper",
        "# W\n\n<Label />\n",
        &[
            (
                "docs/widgets/Label.tsx",
                "import { word } from \"./word\";\nexport default () => <span className=\"label\">{word}</span>;",
            ),
            ("docs/widgets/word.ts", "export const word = \"before\";"),
        ],
    );
    let (mut browser, page) = open(&dir, "/docs/README.md").await;
    assert_eq!(
        text(&page, "document.querySelector('.label').textContent").await,
        "before"
    );
    tokio::time::sleep(Duration::from_millis(20)).await;
    std::fs::write(
        dir.join("docs/widgets/word.ts"),
        "export const word = \"after\";",
    )
    .unwrap();
    page.evaluate("load(location.href)").await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(
        text(&page, "document.querySelector('.label').textContent").await,
        "after"
    );
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn an_edited_widget_rendered_by_name_reaches_open_pages() {
    let dir = widget_folder(
        "by-name",
        "# W\n\n<Card />\n",
        &[
            (
                "docs/widgets/Card.tsx",
                "export default () => <div className=\"card\"><Badge /></div>;",
            ),
            (
                "docs/widgets/Badge.tsx",
                "export default () => <span className=\"badge\">before</span>;",
            ),
        ],
    );
    let (mut browser, page) = open(&dir, "/docs/README.md").await;
    assert_eq!(
        text(&page, "document.querySelector('.card .badge').textContent").await,
        "before"
    );
    tokio::time::sleep(Duration::from_millis(20)).await;
    // Badge's address moves; Card's, which names Badge only by name, doesn't.
    std::fs::write(
        dir.join("docs/widgets/Badge.tsx"),
        "export default () => <span className=\"badge\">after</span>;",
    )
    .unwrap();
    page.evaluate("load(location.href)").await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(
        text(&page, "document.querySelector('.card .badge').textContent").await,
        "after"
    );
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_reload_downloads_nothing_again() {
    let dir = widget_folder(
        "reload",
        "# W\n\n<Label />\n\n<Callout>c</Callout>\n\n```mermaid\nflowchart LR\n  a --> b\n```\n",
        &[
            (
                "docs/widgets/Label.tsx",
                "import { word } from \"./word\";\nexport default () => <span className=\"label\">{word}</span>;",
            ),
            ("docs/widgets/word.ts", "export const word = \"w\";"),
        ],
    );
    let (mut browser, page, answered) = open_logged(&dir, "/docs/README.md").await;
    assert_eq!(
        text(&page, "document.querySelector('.label').textContent").await,
        "w"
    );
    // The comments the page asked for as it became ready have come, so
    // they aren't counted as the reload's.
    for _ in 0..50 {
        if answered
            .lock()
            .unwrap()
            .iter()
            .any(|(path, _)| path == "/__devpage/comments")
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let first = answered.lock().unwrap().len();
    page.reload().await.unwrap();
    testing::loaded(&page).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        text(&page, "document.querySelector('.label').textContent").await,
        "w"
    );
    // After the reload: the page and what may change revalidated (304),
    // everything content-addressed taken from the cache unasked. The event
    // stream reopens, as it must.
    let again: Vec<(String, u16)> = answered.lock().unwrap()[first..]
        .iter()
        .filter(|(path, _)| path != "/__devpage/events" && path != "/favicon.ico")
        .cloned()
        .collect();
    assert!(again.iter().all(|(_, status)| *status == 304), "{again:?}");
    assert!(
        again.iter().any(|(path, _)| path == "/docs/README.md"),
        "the page itself is revalidated: {again:?}"
    );
    assert!(
        !again
            .iter()
            .any(|(path, _)| path.starts_with("/__devpage/widgets/")
                || path.ends_with(".js")
                || path.ends_with(".css")),
        "content-addressed files are never asked for again: {again:?}"
    );
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Samples every frame from now on into `window.sampled`: whether a slide
/// is shown, how many placeholders in the page are not mounted, how much
/// text the page shows, and the scroll position.
const SAMPLER: &str = "window.sampled = []; (function tick() {
        const page = document.getElementById('devpage-page');
        window.sampled.push({
          slide: !!document.querySelector('#devpage-page .slide.shown'),
          unmounted: page.querySelectorAll('[data-widget]:not([data-mounted]):not(style)').length,
          text: page.innerText.trim().length,
          y: scrollY,
        });
        requestAnimationFrame(tick);
      })(); 0";

/// What the server answered since `from`, less the event stream.
fn since(answered: &Answered, from: usize) -> Vec<(String, u16)> {
    answered.lock().unwrap()[from..]
        .iter()
        .filter(|(path, _)| path != "/__devpage/events" && path != "/favicon.ico")
        .cloned()
        .collect()
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_slide_change_fetches_nothing_and_every_frame_shows_a_slide() {
    let dir = widget_folder(
        "slide-change",
        "# W\n",
        &[
            (
                "docs/talk.yml",
                "kind: slides\ntitle: T\nslides: [one.md, two.md, three.md]\n",
            ),
            ("docs/one.md", "# One\n\n<Callout>first</Callout>\n"),
            ("docs/two.md", "# Two\n\nPress <Keys>→</Keys>\n"),
            ("docs/three.md", "# Three\n"),
        ],
    );
    let (mut browser, page, answered) = open_logged(&dir, "/docs/talk.yml").await;
    // The comments the page asked for on load have come.
    tokio::time::sleep(Duration::from_millis(1000)).await;
    page.evaluate(SAMPLER).await.unwrap();
    let from = answered.lock().unwrap().len();
    for key in ["ArrowRight", "ArrowRight", "ArrowLeft"] {
        page.evaluate(format!(
            "document.dispatchEvent(new KeyboardEvent('keydown', {{ key: '{key}' }}))"
        ))
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    page.evaluate("history.back()").await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        text(
            &page,
            "document.querySelector('.slide.shown').dataset.slide + location.hash"
        )
        .await,
        "3#3"
    );
    assert_eq!(
        since(&answered, from),
        vec![],
        "a slide change fetches nothing"
    );
    assert_eq!(
            text(&page, "String(window.sampled.length > 5 && window.sampled.every((f) => f.slide && !f.unmounted))").await,
            "true",
            "every frame shows a slide"
        );
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn an_outline_click_scrolls_without_a_fetch() {
    let long = "A paragraph that takes room.\n\n".repeat(40);
    let dir = widget_folder(
        "outline-click",
        "# W\n",
        &[
            (
                "docs/guide/devpage.yml",
                "kind: doc\ntitle: D\nchapters: [one.md]\n",
            ),
            (
                "docs/guide/one.md",
                &format!("# One\n\n{long}## Far down\n\n{long}"),
            ),
        ],
    );
    let (mut browser, page, answered) = open_logged(&dir, "/docs/guide/devpage.yml").await;
    let from = answered.lock().unwrap().len();
    page.evaluate("[...document.querySelectorAll('.outline a')].find((a) => a.textContent === 'Far down').click()")
            .await
            .unwrap();
    until(&page, "location.hash.endsWith('far-down') && scrollY > 500").await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        since(&answered, from),
        vec![],
        "an outline click fetches nothing"
    );
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_live_update_keeps_unchanged_widgets_their_state_and_the_scroll() {
    let long = "A paragraph that takes room.\n\n".repeat(40);
    let counter = "import { useState, useEffect } from \"react\";\nexport default function Counter() { const [n, setN] = useState(0); useEffect(() => () => { window.unmounted = (window.unmounted ?? 0) + 1; }, []); return <button className=\"counter\" onClick={() => setN((n) => n + 1)}>{n}</button>; }\n";
    let readme =
        |text: &str| format!("# W\n\n{text}\n\n<Counter />\n\n<Callout>{text}</Callout>\n\n{long}");
    let dir = widget_folder(
        "keep",
        &readme("before"),
        &[("docs/widgets/Counter.tsx", counter)],
    );
    let (mut browser, page) = open(&dir, "/docs/README.md").await;
    page.evaluate("document.querySelector('.counter').click(); document.querySelector('.counter').click(); window.counter = document.querySelector('.counter'); scrollTo(0, 600); 0")
            .await
            .unwrap();
    until(
        &page,
        "window.counter.textContent === '2' && scrollY === 600",
    )
    .await;
    page.evaluate(SAMPLER).await.unwrap();

    // The text and the Callout change; the Counter's placeholder doesn't.
    std::fs::write(dir.join("docs/README.md"), readme("after")).unwrap();
    page.evaluate("load(location.href)").await.unwrap();
    until(
        &page,
        "document.querySelector('.callout')?.textContent.includes('after')",
    )
    .await;
    assert_eq!(
            text(&page, "[window.counter === document.querySelector('.counter'), window.counter.textContent, scrollY, window.unmounted ?? 0].join(' ')").await,
            "true 2 600 0",
            "the unchanged widget keeps its node and its state, and the page its scroll"
        );
    assert_eq!(
        text(
            &page,
            "String(window.sampled.every((f) => !f.unmounted && f.text > 0 && f.y === 600))"
        )
        .await,
        "true",
        "no frame shows an unmounted placeholder, an empty page or another scroll"
    );

    // An edited widget gets a new address: it mounts afresh, though its
    // placeholder is the same, and the root it had is unmounted.
    std::fs::write(
        dir.join("docs/widgets/Counter.tsx"),
        counter.replace("className=\"counter\"", "className=\"counter edited\""),
    )
    .unwrap();
    page.evaluate("load(location.href)").await.unwrap();
    until(&page, "!!document.querySelector('.counter.edited')").await;
    assert_eq!(
            text(&page, "[window.counter.isConnected, document.querySelector('.counter').textContent, window.unmounted ?? 0].join(' ')").await,
            "false 0 1"
        );
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn live_updates_during_a_load_make_one_more_load() {
    let dir = widget_folder("coalesce", "# W\n\n<Callout>one</Callout>\n", &[]);
    let (mut browser, page, answered) = open_logged(&dir, "/docs/README.md").await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    std::fs::write(
        dir.join("docs/README.md"),
        "# W\n\n<Callout>two</Callout>\n",
    )
    .unwrap();
    let from = answered.lock().unwrap().len();
    page.evaluate("refresh(); refresh(); refresh(); refresh(); 0")
        .await
        .unwrap();
    until(
        &page,
        "document.querySelector('.callout')?.textContent.includes('two')",
    )
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let pages = since(&answered, from)
        .into_iter()
        .filter(|(path, _)| path == "/docs/README.md")
        .count();
    assert_eq!(pages, 2, "the load, and one more after it");
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_link_swaps_in_a_whole_page_and_back_restores_the_scroll() {
    let long = "A paragraph that takes room.\n\n".repeat(40);
    let dir = widget_folder(
        "navigate",
        &format!("# W\n\n{long}[Other](other.md)\n\n{long}"),
        &[
            (
                "docs/other.md",
                "# Other\n\n<Label />\n\n<Callout>there</Callout>\n",
            ),
            (
                "docs/widgets/Label.tsx",
                "export default () => <span className=\"label\">label</span>;",
            ),
        ],
    );
    let (mut browser, page) = open(&dir, "/docs/README.md").await;
    page.evaluate("scrollTo(0, 900); 0").await.unwrap();
    until(&page, "scrollY === 900").await;
    page.evaluate(SAMPLER).await.unwrap();
    page.evaluate("[...document.querySelectorAll('article a')].find((a) => a.textContent === 'Other').click()")
            .await
            .unwrap();
    until(
        &page,
        "!!document.querySelector('.label') && location.pathname.endsWith('/other.md')",
    )
    .await;
    assert_eq!(text(&page, "String(scrollY)").await, "0");
    assert_eq!(
        text(
            &page,
            "String(window.sampled.every((f) => !f.unmounted && f.text > 0))"
        )
        .await,
        "true",
        "no half-built frame"
    );
    page.evaluate("history.back()").await.unwrap();
    until(
        &page,
        "location.pathname.endsWith('/README.md') && !document.querySelector('.label')",
    )
    .await;
    until(&page, "scrollY === 900").await;
    page.evaluate("history.forward()").await.unwrap();
    until(&page, "!!document.querySelector('.label') && scrollY === 0").await;
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// A site whose tree is two levels deep under *Guide*, with *Fields*
/// under a group heading.
fn sidebar_folder(name: &str) -> PathBuf {
    widget_folder(
        name,
        "# Home\n\n## Topics\n\n- [Guide](guide/README.md)\n\n### Reference\n\n- [Fields](fields.md)\n",
        &[
            (
                "docs/guide/README.md",
                "# Guide\n\n## Topics\n\n- [Step](step.md)\n- [Deep](deep.md)\n",
            ),
            (
                "docs/guide/deep.md",
                "# Deep\n\n## Topics\n\n- [Leaf](leaf.md)\n",
            ),
            ("docs/guide/step.md", "# Step\n"),
            ("docs/guide/leaf.md", "# Leaf\n"),
            ("docs/guide/more.md", "# More\n"),
            (
                "docs/fields.md",
                "# Fields\n\n## Topics\n\n- [Field](field.md)\n",
            ),
            ("docs/field.md", "# Field\n"),
        ],
    )
}

/// Records the sidebar as the server drew it, before the page script
/// runs: the document turns `interactive` before deferred scripts do.
const BEFORE_TAKEOVER: &str = "document.addEventListener('readystatechange', () => {
        if (document.readyState !== 'interactive') return;
        const nav = document.getElementById('devpage-nav');
        const nodes = [nav, ...nav.querySelectorAll('*')];
        window.before = { nodes, boxes: nodes.map((n) => JSON.stringify(n.getBoundingClientRect())), html: nav.outerHTML };
      });";

/// Whether the sidebar is the very DOM the server drew — every node, its
/// markup and its place — now that React has taken it over.
const SAME_AS_BEFORE: &str = "(() => {
        const nav = document.getElementById('devpage-nav');
        const nodes = [nav, ...nav.querySelectorAll('*')];
        return [nodes.length === window.before.nodes.length && nodes.every((n, i) => n === window.before.nodes[i]),
          nav.outerHTML === window.before.html,
          nodes.every((n, i) => JSON.stringify(n.getBoundingClientRect()) === window.before.boxes[i])].join(' ');
      })()";

/// Which branches of the sidebar are open, by label.
const OPEN: &str = "[...document.querySelectorAll('#devpage-nav li.branch.open > .entry > a')].map((a) => a.textContent).join(',')";

/// Clicks the chevron of the sidebar entry labelled `label`.
fn chevron(label: &str) -> String {
    format!(
        "[...document.querySelectorAll('#devpage-nav .chevron')].find((b) => b.getAttribute('aria-label') === '{label}').click(); 0"
    )
}

/// A desktop's window, where the sidebar stands beside the page.
async fn desktop(page: &Page) {
    use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
    page.execute(SetDeviceMetricsOverrideParams::new(1300, 900, 1.0, false))
        .await
        .unwrap();
}

/// Reloads the page, and waits until it is ready.
async fn reload_watching_takeover(page: &Page) {
    page.evaluate("delete document.documentElement.dataset.ready; 0")
        .await
        .unwrap();
    page.reload().await.unwrap();
    until(page, "document.documentElement.dataset.ready === 'true'").await;
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn the_sidebar_is_taken_over_without_a_change_and_its_chevrons_open_and_close() {
    let dir = sidebar_folder("sidebar-takeover");
    let (mut browser, page) = open(&dir, "/docs/guide/deep.md").await;
    desktop(&page).await;
    page.evaluate_on_new_document(BEFORE_TAKEOVER)
        .await
        .unwrap();
    reload_watching_takeover(&page).await;
    // First paint: the sidebar, collapsed, the branch to the page open.
    assert_eq!(
            text(&page, "window.before.nodes.filter((n) => n.matches('li.branch.open')).map((n) => n.querySelector('a').textContent).join(',')").await,
            "Guide,Deep"
        );
    assert_eq!(
        text(
            &page,
            "String(window.before.nodes.some((n) => n.matches('.search input')))"
        )
        .await,
        "true",
        "the search box is drawn with the sidebar"
    );
    assert_eq!(
        text(&page, SAME_AS_BEFORE).await,
        "true true true",
        "React takes the sidebar over without changing it"
    );
    // React re-renders every branch, twice, and writes what it believes
    // is there: the markup it renders is the server's.
    page.evaluate(
        "for (const b of document.querySelectorAll('#devpage-nav .chevron')) b.click(); 0",
    )
    .await
    .unwrap();
    page.evaluate(
        "for (const b of document.querySelectorAll('#devpage-nav .chevron')) b.click(); 0",
    )
    .await
    .unwrap();
    assert_eq!(
        text(
            &page,
            "String(document.getElementById('devpage-nav').outerHTML === window.before.html)"
        )
        .await,
        "true"
    );
    assert_eq!(
            text(&page, "getComputedStyle(document.querySelector('#devpage-nav li[data-href$=\"/fields.md\"] > ul')).display").await,
            "none",
            "a closed branch hides its children"
        );

    // A chevron opens and closes its entry; the link beside it is left.
    page.evaluate(chevron("Fields")).await.unwrap();
    until(&page, &format!("{OPEN} === 'Guide,Deep,Fields'")).await;
    assert_eq!(
            text(&page, "document.querySelector('#devpage-nav li[data-href$=\"/fields.md\"] .chevron').getAttribute('aria-expanded') + ' ' + location.pathname").await,
            "true /docs/guide/deep.md"
        );
    page.evaluate(chevron("Deep")).await.unwrap();
    until(&page, &format!("{OPEN} === 'Guide,Fields'")).await;

    // A reload opens what the reader opened before the first paint, and
    // React takes that over without a change either.
    reload_watching_takeover(&page).await;
    assert_eq!(
            text(&page, "window.before.nodes.filter((n) => n.matches('li.branch.open')).map((n) => n.querySelector('a').textContent).join(',')").await,
            "Guide,Deep,Fields"
        );
    assert_eq!(text(&page, SAME_AS_BEFORE).await, "true true true");
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn the_sidebar_stays_mounted_across_swaps_and_opens_the_branch_navigated_to() {
    let dir = sidebar_folder("sidebar-swap");
    let (mut browser, page, answered) = open_logged(&dir, "/docs/README.md").await;
    desktop(&page).await;
    reload_watching_takeover(&page).await;
    assert_eq!(text(&page, OPEN).await, "");
    page.evaluate(chevron("Fields")).await.unwrap();
    until(&page, &format!("{OPEN} === 'Fields'")).await;
    // Every frame from here on: the sidebar is the same element, in the
    // same place, never empty, and highlights the page the frame shows.
    page.evaluate(
            "window.nav = document.getElementById('devpage-nav'); window.fields = document.querySelector('#devpage-nav li[data-href$=\"/fields.md\"]');
            window.navBox = JSON.stringify(window.nav.getBoundingClientRect());
            window.frames_ = []; (function tick() {
              const nav = document.getElementById('devpage-nav');
              window.frames_.push({
                same: nav === window.nav && JSON.stringify(nav.getBoundingClientRect()) === window.navBox,
                entries: nav.querySelectorAll('a').length,
                agrees: [...nav.querySelectorAll('a.current')].every((a) => a.getAttribute('href') === document.querySelector('#devpage-page article').dataset.source)
                  && nav.querySelectorAll('a.current').length > 0,
              });
              requestAnimationFrame(tick);
            })(); 0",
        )
        .await
        .unwrap();
    let from = answered.lock().unwrap().len();
    page.evaluate("[...document.querySelectorAll('#devpage-nav a')].find((a) => a.textContent === 'Guide').click(); 0")
            .await
            .unwrap();
    until(&page, "location.pathname === '/docs/guide/README.md'").await;
    assert_eq!(text(&page, OPEN).await, "Guide,Fields", "its branch opens");
    // The sidebar follows in the swap's own commit, not a task later.
    assert_eq!(
            text(&page, &format!("load('/docs/guide/deep.md', 'push').then(() => document.querySelector('#devpage-nav a.current').textContent + ' ' + {OPEN})")).await,
            "Deep Guide,Deep,Fields"
        );
    page.evaluate("history.back()").await.unwrap();
    until(&page, "location.pathname === '/docs/guide/README.md'").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
            text(&page, "[window.fields === document.querySelector('#devpage-nav li[data-href$=\"/fields.md\"]'), document.querySelector('#devpage-nav a.current').textContent].join(' ')").await,
            "true Guide",
            "not remounted: the same nodes, the highlight moved"
        );
    assert_eq!(
        text(&page, OPEN).await,
        "Guide,Deep,Fields",
        "open stays open"
    );
    assert_eq!(
            text(
                &page,
                "String(window.frames_.length > 5 && window.frames_.every((f) => f.same && f.entries > 0 && f.agrees))"
            )
            .await,
            "true",
            "no frame moves the sidebar, empties it, or highlights another page than it shows: {}", text(&page, "JSON.stringify(window.frames_.filter((f) => !(f.same && f.entries > 0 && f.agrees)).slice(0, 5)) + window.navBox + JSON.stringify(document.getElementById('devpage-nav').getBoundingClientRect())").await
        );
    let fetched: Vec<_> = since(&answered, from)
        .into_iter()
        .filter(|(path, _)| path.starts_with("/__devpage/"))
        .collect();
    assert!(
        !fetched
            .iter()
            .any(|(path, _)| path.contains("DevpageSidebar")),
        "{fetched:?}"
    );

    // A live update that edits the tree reaches the mounted sidebar. The
    // page it adds is listed nowhere else: a page already in the tree
    // keeps its first, shallowest place.
    std::fs::write(
        dir.join("docs/guide/README.md"),
        "# Guide\n\n## Topics\n\n- [Step](step.md)\n- [Deep](deep.md)\n- [More](more.md)\n",
    )
    .unwrap();
    page.evaluate("load(location.href)").await.unwrap();
    until(
                &page,
                "[...document.querySelectorAll('#devpage-nav li[data-href$=\"/guide/README.md\"] a')].some((a) => a.textContent === 'More')"
            ).await;
    assert_eq!(
            text(&page, "String(window.nav === document.getElementById('devpage-nav') && window.fields === document.querySelector('#devpage-nav li[data-href$=\"/fields.md\"]'))").await,
            "true"
        );
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// A site whose *Retries* page has *The outbox* far down it — the word
/// far down that section, in prose, in code and in a widget — then
/// *Later*, which has it too; another site its Topics reach,
/// *Elsewhere*, titled *Outbox*; and a doc, *Notes*, whose *Drain*
/// section has the word far down it.
fn search_folder(name: &str) -> PathBuf {
    let filler = "Words to scroll past.\n\n".repeat(40);
    widget_folder(
        name,
        "# Home\n\n## Topics\n\n- [Guide](guide.md)\n- [Retries](retries.md)\n- [Elsewhere](../other/devpage.yml)\n",
        &[
            ("docs/guide.md", "# Guide\n\nRead on.\n"),
            (
                "docs/retries.md",
                &format!(
                    "# Retries\n\n{filler}## The outbox\n\n{filler}The client keeps every write in its outbox; `outbox` in code is left alone.\n\n<Callout>An outbox in a widget is left alone.</Callout>\n\n```text\nan outbox in a code block\n```\n\nOutboxes drain in order.\n\n{filler}## Later\n\nAnother outbox, in another section.\n\n{filler}"
                ),
            ),
            (
                "other/devpage.yml",
                "kind: site\ntitle: Elsewhere\nroot: README.md\n",
            ),
            ("other/README.md", "# Outbox\n\nAn outbox, elsewhere.\n"),
            (
                "docs/notes/notes.yml",
                "kind: doc\ntitle: Notes\nchapters: [notes.md]\n",
            ),
            (
                "docs/notes/notes.md",
                &format!("# Notes\n\n## Drain\n\n{filler}The outbox drains.\n\n{filler}"),
            ),
        ],
    )
}

/// The endpoint's answers by query, in ticket 04's shape: `outbox` finds
/// Elsewhere's page by its title, then *Retries*' two sections and two
/// more, then the doc's *Drain*; `retries` finds *Retries* by its title.
const SEARCH_ANSWERS: &str = r#"{
      "outbox": { "pages": [
        { "title": "Outbox", "url": "/other/README.md", "trail": [], "document": { "title": "Elsewhere", "path": "/other/devpage.yml" },
          "kind": "page", "title_match": true, "lede": "An outbox, elsewhere.", "sections": [], "more": 0 },
        { "title": "Retries", "url": "/docs/retries.md", "trail": ["Retries"], "document": { "title": "W", "path": "/docs/devpage.yml" },
          "kind": "page", "title_match": false, "lede": "Words to scroll past.", "more": 2, "sections": [
            { "heading": "The outbox", "path": ["Retries", "The outbox"], "url": "/docs/retries.md#the-outbox",
              "snippet": "The client keeps every write in its <mark>outbox</mark>; …", "words": ["outbox", "Outboxes"] },
            { "heading": "Later", "path": ["Retries", "Later"], "url": "/docs/retries.md#later",
              "snippet": "Another <mark>outbox</mark>, in another section.", "words": ["outbox"] } ] },
        { "title": "Notes", "url": "/docs/notes/notes.yml", "trail": [], "document": { "title": "Notes", "path": "/docs/notes/notes.yml" },
          "kind": "chapter", "title_match": false, "lede": "", "more": 0, "sections": [
            { "heading": "Drain", "path": ["Notes", "Drain"], "url": "/docs/notes/notes.yml#drain",
              "snippet": "The <mark>outbox</mark> drains.", "words": ["outbox"] } ] }
      ] },
      "drain": { "pages": [
        { "title": "Notes", "url": "/docs/notes/notes.yml", "trail": [], "document": { "title": "Notes", "path": "/docs/notes/notes.yml" },
          "kind": "chapter", "title_match": false, "lede": "", "more": 0, "sections": [
            { "heading": "Drain", "path": ["Notes", "Drain"], "url": "/docs/notes/notes.yml#drain",
              "snippet": "The outbox <mark>drains</mark>.", "words": ["drains"] } ] } ] },
      "retries": { "pages": [
        { "title": "Retries", "url": "/docs/retries.md", "trail": ["Retries"], "document": { "title": "W", "path": "/docs/devpage.yml" },
          "kind": "page", "title_match": true, "lede": "Words to scroll past.", "sections": [], "more": 0 } ] }
    }"#;

/// Has the page's searches answered from [`SEARCH_ANSWERS`] — no pages
/// for any other text — until ticket 04's endpoint gives that shape. The
/// text `slow` names is answered after 1.5 s, whether aborted or not.
async fn stub_search(page: &Page, slow: &str) {
    page.evaluate(format!(
            "window.asked = []; const fetch_ = window.fetch; const answers = {SEARCH_ANSWERS};
            window.fetch = (url, options) => {{
              if (!String(url).includes('/__devpage/search')) return fetch_(url, options);
              const q = new URL(url, location.href).searchParams.get('q');
              window.asked.push(q);
              const body = JSON.stringify(answers[q] ?? {{ pages: [] }});
              return new Promise((done) => setTimeout(() => done(new Response(body, {{ headers: {{ 'content-type': 'application/json' }} }})), q === '{slow}' ? 1500 : 0));
            }}; 0"
        ))
        .await
        .unwrap();
}

/// The sidebar's search box.
const SEARCH_INPUT: &str = "#devpage-nav .search input";

/// The result rows shown, in order: each one's kind and its label.
const ROWS: &str = "[...document.querySelectorAll('#devpage-nav .search-results .hit')].map((a) => a.classList[1] + ' ' + a.querySelector('.hit-label').textContent).join('|')";

/// The selected row's label.
const SELECTED: &str =
    "document.querySelector('#devpage-nav .hit.selected .hit-label')?.textContent";

/// The words marked as found in the page, in order.
const FOUND: &str = "[...document.querySelectorAll('#devpage-page mark.devpage-found')].map((m) => m.textContent).join(',')";

/// Types `outbox` in the search box and waits for its results.
async fn search_outbox(page: &Page) -> chromiumoxide::Element {
    let input = page.find_element(SEARCH_INPUT).await.unwrap();
    input.click().await.unwrap();
    input.type_str("outbox").await.unwrap();
    until(page, &format!("{ROWS}.includes('Drain')")).await;
    input
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn search_results_are_grouped_by_page_with_their_sections_under_them() {
    let dir = search_folder("search-typing");
    let (mut browser, page) = open(&dir, "/docs/guide.md").await;
    desktop(&page).await;
    stub_search(&page, "").await;
    let input = search_outbox(&page).await;
    // The tree's place: each page in the server's order, its sections
    // under it, then what else it holds.
    assert_eq!(
        text(&page, ROWS).await,
        "hit-page Outbox|hit-page Retries|hit-section The outbox|hit-section Later|hit-more 2 more in this page|hit-page Notes|hit-section Drain"
    );
    assert_eq!(
            text(&page, "[...document.querySelectorAll('#devpage-nav .search-page')].map((p) => [...p.querySelectorAll('.hit-label')].map((l) => l.textContent).join('/')).join(',')").await,
            "Outbox,Retries/The outbox/Later/2 more in this page,Notes/Drain",
            "a page's rows are one group"
        );
    assert_eq!(
        text(
            &page,
            "getComputedStyle(document.querySelector('#devpage-nav > ul')).display"
        )
        .await,
        "none",
        "the tree is hidden while the box holds text"
    );
    // A page found by its title shows its lede; another document's page
    // says which document it is in; a section's snippet marks the words.
    assert_eq!(
            text(&page, "[...document.querySelectorAll('#devpage-nav .hit-page')].map((a) => [a.querySelector('.hit-place')?.textContent, a.querySelector('.hit-lede')?.textContent].join('/')).join(',')").await,
            "Elsewhere/An outbox, elsewhere.,/,/",
            "a document's title is not said again over a page of that title"
        );
    assert_eq!(
            text(&page, "[...document.querySelectorAll('#devpage-nav .hit-section')[0].querySelectorAll('.hit-snippet mark')].map((m) => m.textContent).join(',')").await,
            "outbox"
        );
    assert_eq!(
            text(&page, "[...document.querySelectorAll('#devpage-nav .search-results .hit')].map((a) => a.getAttribute('href')).join(' ')").await,
            "/other/README.md /docs/retries.md /docs/retries.md#the-outbox /docs/retries.md#later /docs/retries.md /docs/notes/notes.yml /docs/notes/notes.yml#drain",
            "the more row opens the page"
        );
    // The page found by its title is selected first; the arrows move
    // through every row, round from the last to the first.
    assert_eq!(text(&page, SELECTED).await, "Outbox");
    for label in [
        "Retries",
        "The outbox",
        "Later",
        "2 more in this page",
        "Notes",
        "Drain",
        "Outbox",
    ] {
        input.press_key("ArrowDown").await.unwrap();
        until(&page, &format!("{SELECTED} === '{label}'")).await;
    }
    input.press_key("ArrowUp").await.unwrap();
    until(&page, &format!("{SELECTED} === 'Drain'")).await;

    // Escape clears the box and brings the tree back.
    input.press_key("Escape").await.unwrap();
    until(
        &page,
        "document.querySelector('#devpage-nav > ul').checkVisibility()",
    )
    .await;
    assert_eq!(
            text(&page, &format!("[document.querySelector('{SEARCH_INPUT}').value, document.querySelectorAll('#devpage-nav .hit').length, !!document.querySelector('#devpage-nav .search .clear')].join(' ')")).await,
            " 0 false"
        );

    // The clear button clears it as Escape does, leaving the focus in the box.
    input.type_str("outbox").await.unwrap();
    until(
        &page,
        "document.querySelectorAll('#devpage-nav .hit').length > 0",
    )
    .await;
    page.evaluate("document.querySelector('#devpage-nav .search .clear').click(); 0")
        .await
        .unwrap();
    until(
        &page,
        "document.querySelector('#devpage-nav > ul').checkVisibility()",
    )
    .await;
    assert_eq!(
            text(&page, &format!("[document.querySelector('{SEARCH_INPUT}').value, document.querySelectorAll('#devpage-nav .hit').length, document.activeElement === document.querySelector('{SEARCH_INPUT}'), !!document.querySelector('#devpage-nav .search .clear')].join(' ')")).await,
            " 0 true false"
        );

    // Without a page found by its title, its best section is selected first.
    input.type_str("drain").await.unwrap();
    until(
        &page,
        &format!("{ROWS} === 'hit-page Notes|hit-section Drain'"),
    )
    .await;
    assert_eq!(text(&page, SELECTED).await, "Drain");
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Whether the first word marked as found is in view, the heading with
/// the id `heading` above it scrolled past.
fn landed_on_word(heading: &str) -> String {
    format!(
            "(() => {{ const mark = document.querySelector('#devpage-page mark.devpage-found');
              if (!mark) return false;
              const word = mark.getBoundingClientRect();
              return word.top > 0 && word.bottom < innerHeight && document.getElementById('{heading}').getBoundingClientRect().bottom < 0;
            }})()"
        )
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_section_result_lands_on_its_first_word_marked_until_escape() {
    let dir = search_folder("search-landing");
    let (mut browser, page) = open(&dir, "/docs/guide.md").await;
    desktop(&page).await;
    stub_search(&page, "").await;
    page.evaluate("window.nav = document.getElementById('devpage-nav'); window.marker = 1; 0")
        .await
        .unwrap();
    let input = search_outbox(&page).await;
    input.press_key("ArrowDown").await.unwrap();
    input.press_key("ArrowDown").await.unwrap();
    until(&page, &format!("{SELECTED} === 'The outbox'")).await;

    // Enter swaps the page in and lands on the section's first matched
    // word, not its heading; every one of its words in the section is
    // marked — not in code, a widget, the heading or the next section.
    input.press_key("Enter").await.unwrap();
    until(
        &page,
        "location.pathname + location.hash === '/docs/retries.md#the-outbox'",
    )
    .await;
    until(&page, &landed_on_word("the-outbox")).await;
    assert_eq!(text(&page, FOUND).await, "outbox,Outboxes");
    assert_eq!(
            text(&page, "[window.marker, window.nav === document.getElementById('devpage-nav'), document.querySelector('#devpage-nav .search input').value].join(' ')").await,
            "1 true outbox",
            "swapped in, not loaded: the box keeps its text"
        );

    // Scrolled away, Enter lands there again.
    page.evaluate("scrollTo(0, 0); 0").await.unwrap();
    input.press_key("Enter").await.unwrap();
    until(&page, &landed_on_word("the-outbox")).await;
    assert_eq!(text(&page, FOUND).await, "outbox,Outboxes");

    // Escape on the page takes the marks out, leaving the text as it was.
    page.evaluate("document.activeElement.blur(); 0")
        .await
        .unwrap();
    page.find_element("body")
        .await
        .unwrap()
        .press_key("Escape")
        .await
        .unwrap();
    until(&page, &format!("{FOUND} === ''")).await;
    assert_eq!(
            text(&page, "String([...document.querySelectorAll('#devpage-page p')].some((p) => p.textContent === 'Outboxes drain in order.' && p.childNodes.length === 1))").await,
            "true"
        );

    // The next navigation takes them out too: here, the next section.
    input.click().await.unwrap();
    input.press_key("ArrowDown").await.unwrap();
    until(&page, &format!("{SELECTED} === 'Later'")).await;
    input.press_key("Enter").await.unwrap();
    until(
        &page,
        &format!("location.hash === '#later' && {FOUND} === 'outbox'"),
    )
    .await;
    page.evaluate("[...document.querySelectorAll('#devpage-nav a')].find((a) => a.getAttribute('href') === '/docs/retries.md#the-outbox').click(); 0")
            .await
            .unwrap();
    until(
        &page,
        &format!("location.hash === '#the-outbox' && {FOUND} === 'outbox,Outboxes'"),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        text(&page, &format!("String({})", landed_on_word("the-outbox"))).await,
        "true",
        "on the page shown, the #fragment set lands on the word too"
    );
    page.evaluate("history.back(); 0").await.unwrap();
    until(
        &page,
        &format!("location.hash === '#later' && {FOUND} === ''"),
    )
    .await;

    // A doc's chapter loads its document whole, and lands there too.
    page.evaluate("[...document.querySelectorAll('#devpage-nav a')].find((a) => a.getAttribute('href') === '/docs/notes/notes.yml#drain').click(); 0")
            .await
            .unwrap();
    until(&page, "location.pathname === '/docs/notes/notes.yml' && document.documentElement.dataset.ready === 'true'").await;
    until(&page, &landed_on_word("drain")).await;
    assert_eq!(text(&page, FOUND).await, "outbox");
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_title_result_opens_its_page_at_the_top_with_nothing_marked() {
    let dir = search_folder("search-title");
    let (mut browser, page) = open(&dir, "/docs/guide.md").await;
    desktop(&page).await;
    stub_search(&page, "").await;
    let input = search_outbox(&page).await;
    input.press_key("ArrowDown").await.unwrap();
    input.press_key("ArrowDown").await.unwrap();
    input.press_key("Enter").await.unwrap();
    until(&page, &landed_on_word("the-outbox")).await;

    // Found by its title, the page shown opens at its top.
    input.press_key("Escape").await.unwrap();
    input.type_str("retries").await.unwrap();
    until(&page, &format!("{ROWS} === 'hit-page Retries'")).await;
    assert_eq!(text(&page, SELECTED).await, "Retries");
    input.press_key("Enter").await.unwrap();
    until(
        &page,
        "location.pathname + location.hash === '/docs/retries.md' && scrollY === 0",
    )
    .await;
    assert_eq!(text(&page, FOUND).await, "");

    // From another page too.
    page.evaluate("load('/docs/guide.md', 'push')")
        .await
        .unwrap();
    until(&page, "location.pathname === '/docs/guide.md'").await;
    input.press_key("Enter").await.unwrap();
    until(
        &page,
        "location.pathname + location.hash === '/docs/retries.md' && scrollY === 0",
    )
    .await;
    assert_eq!(text(&page, FOUND).await, "");
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn slash_and_command_k_focus_the_sidebar_search() {
    let dir = search_folder("search-keys");
    let (mut browser, page) = open(&dir, "/docs/guide.md").await;
    let focused = format!("document.activeElement === document.querySelector('{SEARCH_INPUT}')");
    assert_eq!(text(&page, &format!("String({focused})")).await, "false");
    page.find_element("body")
        .await
        .unwrap()
        .press_key("/")
        .await
        .unwrap();
    until(&page, &focused).await;
    assert_eq!(
        text(
            &page,
            &format!("document.querySelector('{SEARCH_INPUT}').value")
        )
        .await,
        "",
        "the / focuses, and isn't typed"
    );
    for modifier in ["metaKey", "ctrlKey"] {
        page.evaluate("document.activeElement.blur(); 0")
            .await
            .unwrap();
        assert_eq!(text(&page, &format!("String({focused})")).await, "false");
        page.evaluate(format!(
                "document.body.dispatchEvent(new KeyboardEvent('keydown', {{ key: 'k', {modifier}: true, bubbles: true }})); 0"
            ))
            .await
            .unwrap();
        until(&page, &focused).await;
    }
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_slow_earlier_search_never_replaces_a_later_ones_results() {
    let dir = search_folder("search-stale");
    let (mut browser, page) = open(&dir, "/docs/guide.md").await;
    desktop(&page).await;
    // The search for `retries` answers late, and ignores being aborted.
    stub_search(&page, "retries").await;
    let input = page.find_element(SEARCH_INPUT).await.unwrap();
    input.click().await.unwrap();
    input.type_str("retries").await.unwrap();
    until(&page, "window.asked.includes('retries')").await;
    input.press_key("Escape").await.unwrap();
    search_outbox(&page).await;
    tokio::time::sleep(Duration::from_millis(2000)).await;
    assert!(text(&page, ROWS).await.starts_with("hit-page Outbox|"));
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn comments_are_asked_for_each_page_source_at_once() {
    let dir = widget_folder(
        "comment-sources",
        "# W\n",
        &[
            (
                "docs/talk.yml",
                "kind: slides\ntitle: T\nslides: [one.md, two.md, three.md]\n",
            ),
            (
                "docs/one.md",
                "# One\n\n<Callout>a</Callout>\n\nPress <Keys>→</Keys>\n",
            ),
            ("docs/two.md", "# Two\n\n<Callout>b</Callout>\n"),
            ("docs/three.md", "# Three\n"),
        ],
    );
    let (mut browser, page) = open(&dir, "/docs/talk.yml").await;
    // The comments the page asked for on load have come.
    tokio::time::sleep(Duration::from_millis(1000)).await;
    page.evaluate(
        "window.asked = []; window.open_ = 0; window.peak = 0; const f = window.fetch;
            window.fetch = async (url, options) => {
              if (!String(url).includes('/__devpage/comments')) return f(url, options);
              window.asked.push(decodeURIComponent(String(url).split('page=')[1]));
              window.peak = Math.max(window.peak, ++window.open_);
              try { return await f(url, options); } finally { window.open_--; }
            }; loadComments(); 0",
    )
    .await
    .unwrap();
    until(&page, "window.asked.length >= 3 && window.open_ === 0").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        text(&page, "window.asked.sort().join(',') + ' ' + window.peak").await,
        "/docs/one.md,/docs/three.md,/docs/two.md 3",
        "only the slides, all at once"
    );
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_change_that_leaves_the_page_as_it_was_leaves_the_dom_untouched() {
    let dir = widget_folder("untouched", "# W\n\n<Callout>c</Callout>\n", &[]);
    let (mut browser, page) = open(&dir, "/docs/README.md").await;
    page.evaluate("window.kept = document.querySelector('#devpage-page article'); window.keptWidget = document.querySelector('.callout'); 0")
            .await
            .unwrap();
    // Another file of the site changes; this page renders as it did.
    std::fs::write(dir.join("docs/other.md"), "# Other\n").unwrap();
    page.evaluate("load(location.href)").await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
            text(
                &page,
                "String(window.kept === document.querySelector('#devpage-page article') && window.keptWidget === document.querySelector('.callout'))"
            )
            .await,
            "true"
        );
    // Its own file changes: the content is swapped.
    std::fs::write(dir.join("docs/README.md"), "# W\n\n<Callout>d</Callout>\n").unwrap();
    page.evaluate("load(location.href)").await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
            text(
                &page,
                "String(window.kept === document.querySelector('#devpage-page article')) + ':' + document.querySelector('.callout').textContent.endsWith('d')"
            )
            .await,
            "false:true"
        );
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn failing_widgets_show_why_and_a_clash_keeps_the_system_widget() {
    let dir = widget_folder(
        "failing",
        "# W\n\n<Callout kind=\"tip\">\nThe system one.\n</Callout>\n\n<Counter />\n\n<Nope>hidden</Nope>\n",
        &[
            (
                "docs/widgets/Callout.tsx",
                "export default () => <b className=\"project-callout\">PROJECT</b>;",
            ),
            (
                "docs/widgets/Counter.tsx",
                "import { n } from \"./fmt\";\nexport default () => <span>{n}</span>;",
            ),
            ("docs/widgets/fmt.ts", "export const n = ;"),
        ],
    );
    let (mut browser, page) = open(&dir, "/docs/README.md").await;
    // A project Callout.tsx is a problem; the system Callout still renders.
    assert_eq!(
            text(
                &page,
                "document.querySelector('.callout')?.textContent.includes('The system one') ? 'system' : 'missing'"
            )
            .await,
            "system"
        );
    assert_eq!(
        text(
            &page,
            "String(document.querySelectorAll('.project-callout').length)"
        )
        .await,
        "0"
    );
    let errors = text(
        &page,
        "[...document.querySelectorAll('.widget-error')].map(e => e.textContent).join('\\n')",
    )
    .await;
    // A helper that doesn't compile: its message, in the widget's place.
    assert!(
        errors.contains("<Counter>: docs/widgets/fmt.ts does not compile: line 1:"),
        "{errors}"
    );
    // An unknown tag is an error in its place, its content not shown.
    assert!(errors.contains("<Nope> is not a widget"), "{errors}");
    assert_eq!(
            text(
                &page,
                "document.querySelector('article').textContent.includes('hidden') ? 'shown' : 'not shown'"
            )
            .await,
            "not shown"
        );
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_comment_anchors_on_text_inside_a_widget() {
    let dir = widget_folder(
        "comment",
        "# W\n\n<Callout>\nThe proxy names the reader.\n</Callout>\n",
        &[(
            ".devpage/comments/docs/README.md/20261025T000000Z-0001.yml",
            "anchor:\n  quote: names the reader\n  prefix: 'The proxy '\n  suffix: .\nthread:\n- author: T\n  at: 2026-10-25T00:00:00Z\n  text: which header?\n",
        )],
    );
    let (mut browser, page) = open(&dir, "/docs/README.md").await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        text(
            &page,
            "document.querySelector('aside.callout mark.comment')?.textContent ?? 'none'"
        )
        .await,
        "names the reader"
    );
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}
