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

    let config = BrowserConfig::builder()
        .build()
        .map_err(anyhow::Error::msg)
        .context("finding Chrome")?;
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
    events.abort();
    server.abort();
    Ok(())
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

        let (mut browser, mut handler) = Browser::launch(BrowserConfig::builder().build().unwrap())
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
}
