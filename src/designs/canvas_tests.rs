//! The canvas in Chrome: painting on the GPU and the CPU against the server's
//! PNG, interactions, embedding and printing. They drive the installed Chrome;
//! run them with `cargo test -- --ignored`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chromiumoxide::Page;
use chromiumoxide::browser::Browser;
use chromiumoxide::cdp::browser_protocol::page::{
    AddScriptToEvaluateOnNewDocumentParams, CaptureScreenshotParams, Viewport,
};
use chromiumoxide::layout::Point;
use futures::StreamExt;
use tokio::sync::broadcast;

use crate::document::Document;
use crate::pdf::Profile;
use crate::server;

/// The edge anti-aliasing the GPU's and the CPU's rasterizers may leave
/// between the canvas and the PNG: at most this share of pixels may differ by
/// more than this much in any channel. Measured on the GPU: about 0.01% beyond
/// 4, none beyond 16; the CPU canvas is identical to the PNG.
const CHANNEL_TOLERANCE: u32 = 16;
const DIFFERING_SHARE: f64 = 0.001;

/// A screen of two buttons — Pay navigates, Sheet opens an overlay — a next
/// screen with a back button, and the overlay with a close button. Every box
/// is fixed, so the test knows where each one is.
fn designs(dir: &Path) {
    let files = [
        (
            "button.design",
            r##"<Frame props={{ label: "Button", fill: "#2563eb" }} width={200} height={40} fill={fill} radius={8} layout="row" align="center" justify="center">
  <Text size={16} weight={600} color="#ffffff">{label}</Text>
</Frame>"##,
        ),
        (
            "screen.design",
            r##"import Button from "./button.design"
<Frame width={400} height={300} fill="#f8fafc" layout="column" padding={20} gap={20}>
  <Button label="Pay" onClick="navigate ./next.design" />
  <Button label="Sheet" fill="#16a34a" onClick="overlay ./sheet.design" />
  <Frame width={360} height={100} fill="#e2e8f0" radius={12} shadow={{ y: 4, blur: 12, color: "#0f172a33" }} />
</Frame>"##,
        ),
        (
            "next.design",
            r##"import Button from "./button.design"
<Frame width={400} height={300} fill="#fef3c7" layout="column" padding={20}>
  <Button label="Back" onClick="back" />
</Frame>"##,
        ),
        (
            "sheet.design",
            r##"import Button from "./button.design"
<Frame width={240} height={100} fill="#ffffff" layout="column" padding={20}>
  <Button label="Close" fill="#dc2626" onClick="close" />
</Frame>"##,
        ),
    ];
    for (name, source) in files {
        std::fs::write(dir.join(name), source).unwrap();
    }
}

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("devpage-canvas-{}-{name}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    designs(&dir);
    dir
}

/// Serves `dir`, entered at `entry`; returns its address.
async fn serve(dir: &Path, entry: &str) -> String {
    let document = Arc::new(Document::load(&dir.join(entry), dir).unwrap());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (changes, _) = broadcast::channel(16);
    let router = server::router(document, changes, server::Identity::Local("T".into()));
    tokio::spawn(async move { axum::serve(listener, router).await.ok() });
    format!("http://{address}")
}

/// A headless Chrome with WebGPU.
async fn chrome() -> (Browser, tokio::task::JoinHandle<()>, Profile) {
    let profile = Profile::new();
    let config = profile.chrome().arg("--enable-unsafe-webgpu");
    let (browser, mut handler) = Browser::launch(config.build().unwrap()).await.unwrap();
    let events = tokio::spawn(async move { while handler.next().await.is_some() {} });
    (browser, events, profile)
}

async fn eval<T: serde::de::DeserializeOwned>(page: &Page, js: &str) -> T {
    page.evaluate(js).await.unwrap().into_value().unwrap()
}

/// Waits until `js` is true, for up to 20 seconds.
async fn until(page: &Page, js: &str) {
    for _ in 0..200 {
        if page
            .evaluate(js)
            .await
            .ok()
            .and_then(|v| v.into_value::<bool>().ok())
            .unwrap_or(false)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for {js}");
}

/// Where a design pixel is on the screen: the canvas's view, as the design
/// script records it, from the canvas `selector`.
async fn on_screen(page: &Page, selector: &str, x: f64, y: f64) -> Point {
    let [zoom, vx, vy, left, top]: [f64; 5] = eval(
        page,
        &format!(
            "(() => {{ const c = document.querySelector('{selector}'); const b = c.getBoundingClientRect(); \
             return [...c.dataset.view.split(',').map(Number), b.left, b.top]; }})()"
        ),
    )
    .await;
    Point::new(left + vx + x * zoom, top + vy + y * zoom)
}

/// The design's own 400 × 300 pixels on the canvas page, as a PNG.
async fn screenshot(page: &Page) -> Vec<u8> {
    let at = on_screen(page, "canvas.design", 0.0, 0.0).await;
    let bytes = page
        .screenshot(
            CaptureScreenshotParams::builder()
                .clip(Viewport {
                    x: at.x,
                    y: at.y,
                    width: 400.0,
                    height: 300.0,
                    scale: 1.0,
                })
                .build(),
        )
        .await
        .unwrap();
    bytes
}

/// The color on the screen at `at`, as `[r, g, b]`.
async fn pixel(page: &Page, at: Point) -> [u8; 3] {
    let bytes = page
        .screenshot(
            CaptureScreenshotParams::builder()
                .clip(Viewport {
                    x: at.x.floor(),
                    y: at.y.floor(),
                    width: 1.0,
                    height: 1.0,
                    scale: 1.0,
                })
                .build(),
        )
        .await
        .unwrap();
    let bytes = serde_json::to_string(&bytes).unwrap();
    eval(
        page,
        &format!(
            "(async () => {{
              const bitmap = await createImageBitmap(new Blob([new Uint8Array({bytes})], {{ type: 'image/png' }}));
              const c = new OffscreenCanvas(1, 1);
              const ctx = c.getContext('2d');
              ctx.drawImage(bitmap, 0, 0);
              return [...ctx.getImageData(0, 0, 1, 1).data.slice(0, 3)];
            }})()"
        ),
    )
    .await
}

/// How far the screenshot is from the server's PNG: the largest channel
/// difference, and the share of pixels differing beyond the tolerance.
async fn difference(page: &Page, shot: &[u8], png: &str) -> (u32, f64) {
    let shot = serde_json::to_string(shot).unwrap();
    let [max, share]: [f64; 2] = eval(
        page,
        &format!(
            "(async () => {{
              const pixels = async (blob) => {{
                const bitmap = await createImageBitmap(blob);
                const c = new OffscreenCanvas(bitmap.width, bitmap.height);
                const ctx = c.getContext('2d');
                ctx.drawImage(bitmap, 0, 0);
                return ctx.getImageData(0, 0, bitmap.width, bitmap.height).data;
              }};
              const shot = new Blob([new Uint8Array({shot})], {{ type: 'image/png' }});
              const [a, b] = [await pixels(shot), await pixels(await (await fetch('{png}')).blob())];
              if (a.length !== b.length) return [999, 1];
              let max = 0, over = 0;
              for (let i = 0; i < a.length; i += 4) {{
                let d = 0;
                for (let k = 0; k < 3; k++) d = Math.max(d, Math.abs(a[i + k] - b[i + k]));
                max = Math.max(max, d);
                if (d > {CHANNEL_TOLERANCE}) over++;
              }}
              return [max, over / (a.length / 4)];
            }})()"
        ),
    )
    .await;
    (max as u32, share)
}

async fn canvas_matches_the_png(webgpu: bool) {
    let dir = folder(if webgpu { "gpu" } else { "cpu" });
    let address = serve(&dir, "screen.design").await;
    let (mut browser, events, _profile) = chrome().await;
    let page = browser.new_page("about:blank").await.unwrap();
    if !webgpu {
        // A browser without WebGPU, from the page's first script on.
        page.execute(AddScriptToEvaluateOnNewDocumentParams::new(
            "delete Navigator.prototype.gpu;",
        ))
        .await
        .unwrap();
    }
    page.goto(format!("{address}/screen.design")).await.unwrap();
    until(
        &page,
        "document.querySelector('canvas.design')?.dataset.painted === 'true'",
    )
    .await;
    let backend: String = eval(
        &page,
        "document.querySelector('canvas.design').dataset.backend",
    )
    .await;
    assert_eq!(backend, if webgpu { "gpu" } else { "cpu" });
    // Let the GPU's frame reach the screen.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let shot = screenshot(&page).await;
    let (max, share) = difference(&page, &shot, "/screen.design.png").await;
    eprintln!("{backend}: largest channel difference {max}, {share:.4} of pixels beyond tolerance");
    assert!(
        share <= DIFFERING_SHARE,
        "{backend}: {share} of pixels differ by more than {CHANNEL_TOLERANCE} (largest {max})"
    );
    browser.close().await.ok();
    events.abort();
}

#[tokio::test]
#[ignore]
async fn the_canvas_on_the_gpu_matches_the_png() {
    canvas_matches_the_png(true).await;
}

#[tokio::test]
#[ignore]
async fn the_canvas_on_the_cpu_matches_the_png() {
    canvas_matches_the_png(false).await;
}

#[tokio::test]
#[ignore]
async fn a_click_navigates_back_returns_and_an_overlay_opens_and_closes() {
    let dir = folder("click");
    let address = serve(&dir, "screen.design").await;
    let (mut browser, events, _profile) = chrome().await;
    let page = browser
        .new_page(format!("{address}/screen.design").as_str())
        .await
        .unwrap();
    let painted = "document.querySelector('body > canvas.design')?.dataset.painted === 'true'";
    until(&page, painted).await;

    // The pointer is a hand over a button, and not beside it.
    let pay = on_screen(&page, "body > canvas.design", 120.0, 40.0).await;
    page.move_mouse(pay).await.unwrap();
    until(
        &page,
        "document.querySelector('body > canvas.design').style.cursor === 'pointer'",
    )
    .await;

    // Pay navigates: the address changes and the next design is drawn.
    page.click(pay).await.unwrap();
    until(&page, "location.pathname === '/next.design'").await;
    until(&page, painted).await;
    // Back, on the next screen, returns.
    let back = on_screen(&page, "body > canvas.design", 120.0, 40.0).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    page.click(back).await.unwrap();
    until(&page, "location.pathname === '/screen.design'").await;

    // Sheet opens the overlay, centered; its Close closes it.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let sheet = on_screen(&page, "body > canvas.design", 120.0, 100.0).await;
    page.click(sheet).await.unwrap();
    until(
        &page,
        "document.querySelector('.design-overlay canvas.design')?.dataset.painted === 'true'",
    )
    .await;
    let close = on_screen(&page, ".design-overlay canvas.design", 120.0, 40.0).await;
    page.click(close).await.unwrap();
    until(&page, "!document.querySelector('.design-overlay')").await;
    assert_eq!(
        eval::<String>(&page, "location.pathname").await,
        "/screen.design"
    );
    browser.close().await.ok();
    events.abort();
}

#[tokio::test]
#[ignore]
async fn an_embedded_design_draws_and_survives_a_page_edit() {
    let dir = folder("embed");
    std::fs::write(
        dir.join("page.md"),
        "# Page\n\nThe screen:\n\n![The screen](screen.design)\n",
    )
    .unwrap();
    let address = serve(&dir, "page.md").await;
    let (mut browser, events, _profile) = chrome().await;
    let page = browser
        .new_page(format!("{address}/page.md").as_str())
        .await
        .unwrap();
    until(
        &page,
        "document.querySelector('canvas.design')?.dataset.painted === 'true'",
    )
    .await;
    // At the page's width, as tall as the design is at that width.
    let [width, height]: [f64; 2] = eval(
        &page,
        "(() => { const b = document.querySelector('canvas.design').getBoundingClientRect(); \
         return [b.width, b.height]; })()",
    )
    .await;
    let article: f64 = eval(
        &page,
        "document.querySelector('article').getBoundingClientRect().width",
    )
    .await;
    assert!((width - article).abs() < 1.0, "{width} vs {article}");
    assert!((height - width * 0.75).abs() < 1.0, "{height} for {width}");

    // Clicking Pay navigates inside the embed, not the page: the next
    // design's yellow replaces the screen's grey.
    page.evaluate("document.querySelector('canvas.design').marked = true")
        .await
        .unwrap();
    // Beside the Pay button: the design's own background.
    let corner = on_screen(&page, "canvas.design", 300.0, 40.0).await;
    let grey = pixel(&page, corner).await;
    assert_eq!(grey, [0xf8, 0xfa, 0xfc]);
    let pay = on_screen(&page, "canvas.design", 120.0, 40.0).await;
    page.click(pay).await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(eval::<String>(&page, "location.pathname").await, "/page.md");
    assert_eq!(pixel(&page, corner).await, [0xfe, 0xf3, 0xc7]);

    // A page edit swaps the text and keeps the very same canvas.
    std::fs::write(
        dir.join("page.md"),
        "# Page\n\nThe screen, edited:\n\n![The screen](screen.design)\n",
    )
    .unwrap();
    page.evaluate("load(location.href)").await.unwrap();
    until(
        &page,
        "document.querySelector('article').textContent.includes('edited')",
    )
    .await;
    assert!(
        eval::<bool>(
            &page,
            "document.querySelector('canvas.design').marked === true"
        )
        .await
    );
    browser.close().await.ok();
    events.abort();
}

#[tokio::test]
#[ignore]
async fn a_doc_prints_its_embedded_design() {
    let dir = folder("print");
    std::fs::write(
        dir.join("doc.md"),
        "# Doc\n\n![The screen](screen.design)\n",
    )
    .unwrap();
    let document = Arc::new(Document::load(&dir.join("doc.md"), &dir).unwrap());
    let with = dir.join("with.pdf");
    crate::pdf::print(document, &with).await.unwrap();
    std::fs::write(dir.join("plain.md"), "# Doc\n\nNo design.\n").unwrap();
    let document = Arc::new(Document::load(&dir.join("plain.md"), &dir).unwrap());
    let without = dir.join("without.pdf");
    crate::pdf::print(document, &without).await.unwrap();
    // The painted canvas is an image in the PDF; the page without one has none.
    let images = |path: &Path| {
        let bytes = std::fs::read(path).unwrap();
        bytes.windows(13).filter(|w| w == b"/Subtype /Ima").count()
    };
    assert!(images(&with) > 0, "no image in the printed doc");
    assert_eq!(images(&without), 0);
}

#[tokio::test]
#[ignore]
async fn the_wheel_zooms_about_the_pointer_and_pans() {
    let dir = folder("zoom");
    let address = serve(&dir, "screen.design").await;
    let (mut browser, events, _profile) = chrome().await;
    let page = browser
        .new_page(format!("{address}/screen.design").as_str())
        .await
        .unwrap();
    until(
        &page,
        "document.querySelector('canvas.design')?.dataset.painted === 'true'",
    )
    .await;
    let view = || async {
        eval::<String>(
            &page,
            "document.querySelector('canvas.design').dataset.view",
        )
        .await
    };
    // Fit to the window: 400 × 300 at 100%, centered.
    let fitted = view().await;
    let [zoom, x, y]: [f64; 3] = serde_json::from_str(&format!("[{fitted}]")).unwrap();
    assert_eq!(zoom, 1.0);
    assert!(x > 0.0 && y > 0.0, "{fitted}");

    // ⌘ and the wheel zooms in, keeping the design point under the pointer.
    let pointer = on_screen(&page, "canvas.design", 100.0, 100.0).await;
    page.evaluate(format!(
        "document.querySelector('canvas.design').dispatchEvent(new WheelEvent('wheel', \
         {{ deltaY: -100, metaKey: true, clientX: {}, clientY: {}, bubbles: true, cancelable: true }}))",
        pointer.x, pointer.y
    ))
    .await
    .unwrap();
    until(
        &page,
        "document.querySelector('canvas.design').dataset.view.split(',')[0] > 2",
    )
    .await;
    let after = on_screen(&page, "canvas.design", 100.0, 100.0).await;
    assert!((after.x - pointer.x).abs() < 0.5 && (after.y - pointer.y).abs() < 0.5);

    // The wheel alone pans.
    page.evaluate(
        "document.querySelector('canvas.design').dispatchEvent(new WheelEvent('wheel', \
         { deltaX: 40, deltaY: 0, bubbles: true, cancelable: true }))",
    )
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let panned = on_screen(&page, "canvas.design", 100.0, 100.0).await;
    assert!((panned.x - (after.x - 40.0)).abs() < 0.5);
    browser.close().await.ok();
    events.abort();
}
