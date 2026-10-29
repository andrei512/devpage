//! `devpage pdf` in chrome-headless-shell: a doc printed on its paper with
//! its header and footer, a deck one 16:9 page per slide, never held up by a
//! broken design, and each widget's first render on paper.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use super::page_script::{open, text, widget_folder};
use crate::drive::pdf::print;
use devpage_core::document::{Document, Kind};
use devpage_core::folder::ServedFolder;

/// The pages of a PDF Chrome wrote: each one's width and height in points.
fn pages(pdf: &[u8]) -> Vec<(f64, f64)> {
    let text = String::from_utf8_lossy(pdf);
    text.match_indices("/MediaBox [")
        .map(|(at, _)| {
            let numbers: Vec<f64> = text[at + 11..]
                .split(']')
                .next()
                .unwrap()
                .split_whitespace()
                .map(|n| n.parse().unwrap())
                .collect();
            (numbers[2] - numbers[0], numbers[3] - numbers[1])
        })
        .collect()
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_design_with_problems_does_not_delay_a_pdf() {
    let dir = std::env::temp_dir().join(format!("devpage-pdf-problems-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("broken.design"), "<Frame width={400} height={300}").unwrap();
    std::fs::write(dir.join("doc.md"), "# Doc\n\n![Broken](broken.design)\n").unwrap();
    let dir = dir.canonicalize().unwrap();
    let document =
        Arc::new(Document::load(&dir.join("doc.md"), &ServedFolder::new(&dir).unwrap()).unwrap());
    let started = std::time::Instant::now();
    let pdf = print(document).await.unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert!(!pages(&pdf).is_empty());
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn the_fixture_doc_prints_on_a4() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let document = Arc::new(
        Document::load(
            &root.join("tests/fixtures/sync-design"),
            &ServedFolder::new(root).unwrap(),
        )
        .unwrap(),
    );
    // Its print call — the paper, the header and the footer — is
    // `pdf::tests::a_docs_print_call_carries_its_paper_header_and_footer`.
    let pages = pages(&print(document).await.unwrap());
    assert!(!pages.is_empty());
    for (width, height) in pages {
        // A4 is 595 × 842 points.
        assert!(
            (width - 595.0).abs() < 2.0 && (height - 842.0).abs() < 2.0,
            "{width} × {height}"
        );
    }
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn the_talk_prints_one_16_by_9_page_per_slide() {
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
    let count = slides.len();
    let pages = pages(&print(document).await.unwrap());
    assert_eq!(pages.len(), count, "{pages:?}");
    for (width, height) in pages {
        assert!(
            (width / height - 16.0 / 9.0).abs() < 0.01,
            "{width} × {height}"
        );
    }
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_printed_doc_shows_each_widgets_first_render() {
    let dir = widget_folder(
        "print",
        "# W\n",
        &[
            (
                "docs/design/devpage.yml",
                "kind: doc\ntitle: D\nchapters: [one.md]\n",
            ),
            (
                "docs/design/one.md",
                "# One\n\n<Tabs>\n<Tab title=\"First\">\n\nshown on paper\n\n</Tab>\n<Tab title=\"Second\">\n\nonly on screen\n\n</Tab>\n</Tabs>\n\n<Terminal>\n\n```\n$ echo a long line\n```\n\n</Terminal>\n\n```mermaid\nflowchart LR\n  paper[On paper] --> web\n  click paper href \"https://example.com/drawn-on-the-server\"\n```\n",
            ),
        ],
    );
    let (mut browser, page) = open(&dir, "/docs/design/devpage.yml?print").await;
    assert_eq!(
            text(&page, "document.documentElement.dataset.ready + ':' + document.querySelector('.tabs-panel').textContent.trim() + ':' + document.body.textContent.includes('only on screen')").await,
            "true:shown on paper:false"
        );
    // On paper a terminal's long lines wrap rather than scroll.
    page.execute(
        chromiumoxide::cdp::browser_protocol::emulation::SetEmulatedMediaParams::builder()
            .media("print")
            .build(),
    )
    .await
    .unwrap();
    assert_eq!(
        text(
            &page,
            "getComputedStyle(document.querySelector('.terminal pre')).whiteSpace"
        )
        .await,
        "pre-wrap"
    );
    // The diagram is on paper, drawn by the server.
    assert_eq!(
        text(
            &page,
            "String(document.querySelector('.diagram svg')?.getBoundingClientRect().width > 0)"
        )
        .await,
        "true"
    );
    browser.close().await.ok();
    let document = Arc::new(
        Document::load(&dir.join("docs/design"), &ServedFolder::new(&dir).unwrap()).unwrap(),
    );
    let pdf = print(document).await.unwrap();
    assert!(!pages(&pdf).is_empty());
    // Its node's link is the PDF's too.
    assert!(
        String::from_utf8_lossy(&pdf).contains("https://example.com/drawn-on-the-server"),
        "the diagram's link in the PDF"
    );
    std::fs::remove_dir_all(&dir).ok();
}
