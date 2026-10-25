//! `devpage pdf`: Chrome prints the page devpage serves, in print mode, so the
//! PDF is the page the browser shows.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::page::PrintToPdfParams;
use futures::StreamExt;
use tokio::sync::broadcast;

use crate::document::{Document, Kind, Paper, Pdf};
use crate::server;

/// Prints `document` to `output`.
pub async fn print(document: Arc<Document>, output: &Path) -> Result<()> {
    let params = match &document.kind {
        Kind::Doc { pdf, .. } => doc_params(&document.title, pdf),
        Kind::Slides { .. } => PrintToPdfParams {
            print_background: Some(true),
            prefer_css_page_size: Some(true),
            paper_width: Some(20.0),
            paper_height: Some(11.25),
            margin_top: Some(0.0),
            margin_bottom: Some(0.0),
            margin_left: Some(0.0),
            margin_right: Some(0.0),
            ..Default::default()
        },
        Kind::Site { .. } => bail!("pdf prints a doc or slides, not a site"),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (changes, _) = broadcast::channel(1);
    let url = format!("http://{address}{}?print", document.entry());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            server::router(document, changes, server::Identity::Local(String::new())),
        )
        .await
        .ok();
    });

    let profile = Profile::new();
    let config = match profile.chrome().build() {
        Ok(config) => config,
        // No Chrome installed: download a Chromium once, into devpage's cache.
        Err(_) => profile
            .chrome()
            .chrome_executable(fetch_chromium().await?)
            .build()
            .map_err(anyhow::Error::msg)?,
    };
    let (mut browser, mut handler) = Browser::launch(config).await.context("starting Chrome")?;
    let events = tokio::spawn(async move { while handler.next().await.is_some() {} });

    let page = browser.new_page(url.as_str()).await?;
    // devpage.js marks the page ready once its diagrams are drawn.
    for _ in 0..100 {
        let ready: bool = page
            .evaluate("document.documentElement.dataset.ready === 'true'")
            .await?
            .into_value()?;
        if ready {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let bytes = page.pdf(params).await.context("printing")?;
    std::fs::write(output, bytes).with_context(|| format!("writing {}", output.display()))?;

    browser.close().await.ok();
    browser.wait().await.ok();
    events.abort();
    server.abort();
    Ok(())
}

/// The file `devpage pdf` writes when `-o` names none: the document's title,
/// with the characters a file name can't hold replaced by `-`.
pub fn default_output(title: &str) -> std::path::PathBuf {
    let name: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    let name = name.trim().trim_start_matches('.');
    let name = if name.is_empty() { "document" } else { name };
    std::path::PathBuf::from(format!("{name}.pdf"))
}

/// A Chrome profile folder of its own for one run, so several prints can run
/// at once without fighting over one profile's lock. It is deleted when the
/// run ends.
struct Profile(std::path::PathBuf);

impl Profile {
    fn new() -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static RUNS: AtomicUsize = AtomicUsize::new(0);
        Self(std::env::temp_dir().join(format!(
            "devpage-chrome-{}-{}",
            std::process::id(),
            RUNS.fetch_add(1, Ordering::Relaxed)
        )))
    }

    /// A headless Chrome using this profile.
    fn chrome(&self) -> chromiumoxide::browser::BrowserConfigBuilder {
        BrowserConfig::builder().user_data_dir(&self.0)
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// Downloads a Chromium into devpage's cache folder, or finds the one there.
async fn fetch_chromium() -> Result<std::path::PathBuf> {
    use chromiumoxide::fetcher::{BrowserFetcher, BrowserFetcherOptions};
    let home = std::env::var_os("HOME").context("no HOME for devpage's cache folder")?;
    let cache = if cfg!(target_os = "macos") {
        Path::new(&home).join("Library/Caches/devpage/chromium")
    } else {
        Path::new(&home).join(".cache/devpage/chromium")
    };
    std::fs::create_dir_all(&cache)?;
    eprintln!("No Chrome found; using a Chromium in {}", cache.display());
    let fetcher = BrowserFetcher::new(
        BrowserFetcherOptions::builder()
            .with_path(&cache)
            .build()
            .map_err(anyhow::Error::msg)?,
    );
    let info = fetcher.fetch().await.context("downloading Chromium")?;
    Ok(info.executable_path)
}

/// The print call for a doc: its paper, and its header and footer, whose
/// `{page}` and `{pages}` become Chrome's page-number fields.
fn doc_params(title: &str, pdf: &Pdf) -> PrintToPdfParams {
    let template = |text: &str| {
        let text = text
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace("{page}", "<span class=\"pageNumber\"></span>")
            .replace("{pages}", "<span class=\"totalPages\"></span>");
        format!(
            "<div style=\"font: 9px -apple-system, Helvetica, sans-serif; color: #6e6e73; width: 100%; text-align: center;\">{text}</div>"
        )
    };
    let (width, height) = match pdf.paper {
        Paper::A4 => (8.27, 11.69),
        Paper::Letter => (8.5, 11.0),
    };
    let _ = title;
    PrintToPdfParams {
        display_header_footer: Some(true),
        header_template: Some(template(pdf.header.as_deref().unwrap_or(""))),
        footer_template: Some(template(&pdf.footer)),
        print_background: Some(true),
        prefer_css_page_size: Some(true),
        paper_width: Some(width),
        paper_height: Some(height),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_file_name_is_the_title_made_safe() {
        assert_eq!(
            default_output("Offline sync"),
            Path::new("Offline sync.pdf")
        );
        assert_eq!(default_output("A/B: why?"), Path::new("A-B- why-.pdf"));
        assert_eq!(default_output("..."), Path::new("document.pdf"));
    }

    #[tokio::test]
    async fn a_site_is_refused() {
        let dir = std::env::temp_dir().join(format!("devpage-pdf-site-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("README.md"), "# Home\n").unwrap();
        std::fs::write(
            dir.join("devpage.yml"),
            "kind: site\ntitle: Home\nroot: README.md\n",
        )
        .unwrap();
        let site = Arc::new(Document::load(&dir).unwrap());
        let error = print(site, Path::new("never.pdf")).await.unwrap_err();
        assert_eq!(error.to_string(), "pdf prints a doc or slides, not a site");
        assert!(!Path::new("never.pdf").exists());
    }

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

    /// Drives the installed Chrome; run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn the_example_doc_prints_on_a4_with_its_header_and_footer() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let document = Arc::new(Document::load(&root.join("examples/sync-design")).unwrap());
        let Kind::Doc { pdf, .. } = &document.kind else {
            panic!()
        };
        let params = doc_params(&document.title, pdf);
        assert!(
            params
                .header_template
                .unwrap()
                .contains("Offline sync — draft")
        );
        let footer = params.footer_template.unwrap();
        assert!(
            footer.contains(
                "Page <span class=\"pageNumber\"></span> of <span class=\"totalPages\"></span>"
            ),
            "{footer}"
        );

        let out = std::env::temp_dir().join(format!("devpage-doc-{}.pdf", std::process::id()));
        print(document, &out).await.unwrap();
        let pages = pages(&std::fs::read(&out).unwrap());
        assert!(!pages.is_empty());
        for (width, height) in pages {
            // A4 is 595 × 842 points.
            assert!(
                (width - 595.0).abs() < 2.0 && (height - 842.0).abs() < 2.0,
                "{width} × {height}"
            );
        }
    }

    /// Drives the installed Chrome; run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn the_example_deck_prints_one_16_by_9_page_per_slide() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let document = Arc::new(Document::load(&root.join("examples/launch-talk")).unwrap());
        let Kind::Slides { slides } = &document.kind else {
            panic!()
        };
        let count = slides.len();
        let out = std::env::temp_dir().join(format!("devpage-deck-{}.pdf", std::process::id()));
        print(document, &out).await.unwrap();
        let pages = pages(&std::fs::read(&out).unwrap());
        assert_eq!(pages.len(), count, "{pages:?}");
        for (width, height) in pages {
            assert!(
                (width / height - 16.0 / 9.0).abs() < 0.01,
                "{width} × {height}"
            );
        }
    }

    /// Drives the installed Chrome; run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn comments_show_on_the_page_and_detach_when_their_text_is_gone() {
        let dir = std::env::temp_dir().join(format!("devpage-chrome-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(
            dir.join("one.md"),
            "# One\n\nThe client retries every write.\n",
        )
        .unwrap();
        let document = Arc::new(Document::load(&dir.join("one.md")).unwrap());
        let store = crate::comments::Store::new(&document.repo);
        let anchor = |quote: &str| crate::comments::Anchor {
            quote: quote.into(),
            prefix: String::new(),
            suffix: String::new(),
        };
        store
            .create("/one.md", anchor("retries"), "Ana", "Every write?")
            .unwrap();
        store
            .create("/one.md", anchor("long gone"), "Bo", "Old")
            .unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (changes, _) = broadcast::channel(1);
        let router = server::router(document, changes, server::Identity::Local("T".into()));
        tokio::spawn(async move { axum::serve(listener, router).await.ok() });

        let profile = Profile::new();
        let (mut browser, mut handler) = Browser::launch(profile.chrome().build().unwrap())
            .await
            .unwrap();
        let events = tokio::spawn(async move { while handler.next().await.is_some() {} });
        let page = browser
            .new_page(format!("http://{address}/one.md").as_str())
            .await
            .unwrap();
        let mut found = (0, String::new());
        for _ in 0..40 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let marks: i64 = page
                .evaluate("document.querySelectorAll('mark.comment').length")
                .await
                .unwrap()
                .into_value()
                .unwrap();
            let panel: String = page
                .evaluate("document.getElementById('comments')?.innerText ?? ''")
                .await
                .unwrap()
                .into_value()
                .unwrap();
            found = (marks, panel);
            if found.0 == 1 && found.1.contains("Detached") {
                break;
            }
        }
        browser.close().await.ok();
        events.abort();
        assert_eq!(found.0, 1, "{found:?}");
        assert!(
            found.1.contains("Detached comments") && found.1.contains("Every write?"),
            "{found:?}"
        );
    }

    /// Drives the installed Chrome; run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn a_deck_shows_one_slide_and_moves_with_the_arrow_keys() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let document = Arc::new(Document::load(&root.join("examples/launch-talk")).unwrap());
        let url = document.entry();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (changes, _) = broadcast::channel(1);
        let router = server::router(document, changes, server::Identity::Local("T".into()));
        tokio::spawn(async move { axum::serve(listener, router).await.ok() });
        let profile = Profile::new();
        let (mut browser, mut handler) = Browser::launch(profile.chrome().build().unwrap())
            .await
            .unwrap();
        let events = tokio::spawn(async move { while handler.next().await.is_some() {} });
        let page = browser
            .new_page(format!("http://{address}{url}").as_str())
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        let shown = |page: chromiumoxide::Page| async move {
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
        assert_eq!(shown(page.clone()).await, "3#3");
        // ⌘← is the browser's, not the deck's.
        page.evaluate("document.dispatchEvent(new KeyboardEvent('keydown', {key: 'ArrowLeft', metaKey: true}))")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(shown(page.clone()).await, "3#3");
        // A live update swaps the deck in place and stays on the slide shown.
        page.evaluate("load(location.href)").await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(shown(page.clone()).await, "3#3");
        browser.close().await.ok();
        events.abort();
    }

    /// Drives the installed Chrome; run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn a_docs_outline_moves_above_the_text_on_a_narrow_screen() {
        use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let document = Arc::new(Document::load(&root.join("examples/sync-design")).unwrap());
        let url = document.entry();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (changes, _) = broadcast::channel(1);
        let router = server::router(document, changes, server::Identity::Local("T".into()));
        tokio::spawn(async move { axum::serve(listener, router).await.ok() });
        let profile = Profile::new();
        let (mut browser, mut handler) = Browser::launch(profile.chrome().build().unwrap())
            .await
            .unwrap();
        let events = tokio::spawn(async move { while handler.next().await.is_some() {} });
        let page = browser.new_page("about:blank").await.unwrap();
        let layout = |width: i64| {
            let page = page.clone();
            let url = format!("http://{address}{url}");
            async move {
                page.execute(SetDeviceMetricsOverrideParams::new(width, 900, 1.0, false))
                    .await
                    .unwrap();
                page.goto(url.as_str()).await.unwrap();
                tokio::time::sleep(Duration::from_millis(300)).await;
                let value: String = page
                    .evaluate("(() => { const n = document.getElementById('nav').getBoundingClientRect(), a = document.querySelector('article').getBoundingClientRect(); return n.bottom <= a.top ? 'above' : n.right <= a.left ? 'beside' : 'overlap'; })()")
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
        events.abort();
    }
}
