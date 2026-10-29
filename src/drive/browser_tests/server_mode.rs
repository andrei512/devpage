//! A published document in chrome-headless-shell: every request its pages
//! make stays under their mount.

use std::time::Duration;

use chromiumoxide::Browser;

use super::{publish, server_mode, zip_of};
use crate::chrome::testing::{self, Answered, eval, until};

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

/// Whether `path` is a built-in asset's address: the same for every
/// document, so outside any mount.
fn built_in(path: &str) -> bool {
    path.strip_prefix("/__devpage/").is_some_and(|name| {
        devpage_core::addresses::asset(name).is_some_and(|(_, current)| current)
    })
}

/// Opens `mount`'s root page and uses everything on it, then checks that
/// every request the page made was answered, under `mount`.
async fn every_request_stays_under(browser: &Browser, base: &str, heard: &Answered, mount: &str) {
    heard.lock().unwrap().clear();
    let page = testing::open(browser, &format!("{base}{mount}/README.md")).await;
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
    let (router, heard) = testing::answering(server_mode("mounted"));
    let base = testing::serve(router).await;
    let id = publish(&base, None, site()).await;

    let browser = testing::launch().await;

    // The latest, in place at the link, and the same version at its own
    // address.
    every_request_stays_under(&browser, &base, &heard, &format!("/d/{id}")).await;
    every_request_stays_under(&browser, &base, &heard, &format!("/d/{id}/v/1")).await;

    drop(browser);
}
