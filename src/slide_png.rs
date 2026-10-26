//! A slide as an image: `<deck>.yml/<n>.png` is slide `n` of the deck, as the
//! browser devpage drives shows it on a 1920 × 1080 screen — so an agent
//! writing a slide's notes can look at the slide.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use chromiumoxide::Page;
use chromiumoxide::browser::Browser;
use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotParams;
use futures::StreamExt;
use tokio::sync::broadcast;

use crate::document::Document;
use crate::pdf::{Profile, READY};
use crate::server;

/// devpage's own controls, which are not part of the slide.
const HIDE_CONTROLS: &str = "document.head.insertAdjacentHTML('beforeend', \
    '<style>#devpage-corner, #devpage-comments-toggle { display: none !important; }</style>')";

/// Slide `n` (1-based) of `deck`, as a 1920 × 1080 PNG. The deck is served on
/// its own for the one Chrome that draws it; both end with the call.
pub async fn png(deck: Arc<Document>, n: usize) -> Result<Vec<u8>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}{}#{n}", listener.local_addr()?, deck.entry());
    let (changes, _) = broadcast::channel(1);
    let server = tokio::spawn(async move {
        axum::serve(listener, server::printing(deck, changes))
            .await
            .ok();
    });
    let profile = Profile::new();
    // With the GPU on, a screenshot holds an embedded design's WebGPU canvas
    // as the reader sees it, rather than black.
    let config = profile
        .chrome()
        .await?
        .arg("enable-unsafe-webgpu")
        .arg("enable-gpu")
        .build()
        .map_err(anyhow::Error::msg)?;
    let (mut browser, mut handler) = Browser::launch(config).await.context("starting Chrome")?;
    let events = tokio::spawn(async move { while handler.next().await.is_some() {} });
    let shot = capture(&browser, &url).await;
    browser.close().await.ok();
    browser.wait().await.ok();
    events.abort();
    server.abort();
    shot
}

/// Opens `url` on a 1920 × 1080 screen, waits until the page is ready, and
/// screenshots it without devpage's controls.
async fn capture(browser: &Browser, url: &str) -> Result<Vec<u8>> {
    let page: Page = browser.new_page("about:blank").await?;
    page.execute(SetDeviceMetricsOverrideParams::new(1920, 1080, 1.0, false))
        .await?;
    page.goto(url).await?;
    for _ in 0..400 {
        let ready: bool = page.evaluate(READY).await?.into_value()?;
        if ready {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    page.evaluate(HIDE_CONTROLS).await?;
    page.screenshot(CaptureScreenshotParams::builder().build())
        .await
        .context("taking the screenshot")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn a_slide_is_served_as_a_1920_by_1080_png() {
        let dir = std::env::temp_dir().join(format!("devpage-slide-png-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("one.md"), "# One\n").unwrap();
        std::fs::write(dir.join("two.md"), "# Two\n\n<Notes>Never drawn.</Notes>\n").unwrap();
        std::fs::write(
            dir.join("talk.yml"),
            "kind: slides\ntitle: Talk\nslides: [one.md, two.md]\n",
        )
        .unwrap();
        let dir = dir.canonicalize().unwrap();
        let document = Arc::new(Document::load(&dir.join("talk.yml"), &dir).unwrap());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (changes, _) = broadcast::channel(1);
        let router = server::router(document, changes, server::Identity::Local("T".into()));
        tokio::spawn(async move { axum::serve(listener, router).await.ok() });

        let response = reqwest::get(format!("http://{address}/talk.yml/2.png"))
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["content-type"], "image/png");
        let png = response.bytes().await.unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        // The IHDR chunk's width and height, big-endian.
        let size = |at: usize| u32::from_be_bytes(png[at..at + 4].try_into().unwrap());
        assert_eq!((size(16), size(20)), (1920, 1080));

        // Slide 1 and slide 2 are different pictures.
        let first = reqwest::get(format!("http://{address}/talk.yml/1.png"))
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        assert_ne!(first, png);

        for path in ["talk.yml/3.png", "talk.yml/0.png", "one.md/1.png"] {
            let status = reqwest::get(format!("http://{address}/{path}"))
                .await
                .unwrap()
                .status();
            assert_eq!(status, 404, "{path}");
        }
    }
}
