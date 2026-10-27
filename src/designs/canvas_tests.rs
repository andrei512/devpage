//! The canvas in Chrome: painting on the GPU and the CPU against the server's
//! PNG, interactions, embedding and printing. They drive chrome-headless-shell;
//! run them with `cargo test -- --ignored`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chromiumoxide::Page;
use chromiumoxide::browser::Browser;
use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
use chromiumoxide::cdp::browser_protocol::page::{
    AddScriptToEvaluateOnNewDocumentParams, CaptureScreenshotParams, Viewport,
};
use chromiumoxide::layout::Point;
use futures::StreamExt;
use tokio::sync::broadcast;

use devpage_design::render::{self, Difference};

use crate::document::Document;
use crate::pdf::Profile;
use crate::server;

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
    let router = server::router(server::Options {
        comments: Some(server::Identity::Local("T".into())),
        ..server::Options::new(document, changes)
    });
    tokio::spawn(async move { axum::serve(listener, router).await.ok() });
    format!("http://{address}")
}

/// Serves `dir`, entered at `entry`, and hands back its design store, so a
/// test can tell it about a saved file as the watcher would.
async fn serve_designs(dir: &Path, entry: &str) -> (String, super::Designs) {
    let document = Arc::new(Document::load(&dir.join(entry), dir).unwrap());
    let designs = super::Designs::new(dir.to_path_buf());
    let changes = broadcast::channel(16).0;
    let router = server::router(server::Options {
        designs: designs.clone(),
        comments: Some(server::Identity::Local("T".into())),
        preview: crate::check::testing::preview(),
        ..server::Options::new(document, changes)
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.ok() });
    (format!("http://{address}"), designs)
}

/// A headless Chrome with WebGPU. chrome-headless-shell keeps its GPU
/// process off unless `enable-gpu` is passed; with it, WebGPU gets the real
/// adapter and screenshots include the WebGPU canvas as the reader sees it,
/// composited — without it they hold black where the canvas is. chromiumoxide
/// adds the `--` itself.
async fn chrome() -> (Browser, tokio::task::JoinHandle<()>, Profile) {
    let profile = Profile::new();
    let config = profile
        .chrome()
        .await
        .unwrap()
        .arg("enable-unsafe-webgpu")
        .arg("enable-gpu");
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

/// The design's drawing, `width` × `height` pixels, on the canvas page and
/// `margin` pixels around it, as a PNG.
async fn screenshot(page: &Page, width: f64, height: f64, margin: f64) -> Vec<u8> {
    let at = on_screen(page, "canvas.design", -margin, -margin).await;
    page.screenshot(
        CaptureScreenshotParams::builder()
            .clip(Viewport {
                x: at.x,
                y: at.y,
                width: width + 2.0 * margin,
                height: height + 2.0 * margin,
                scale: 1.0,
            })
            .build(),
    )
    .await
    .unwrap()
}

/// The color on the screen at `at`, as `[r, g, b]`.
/// Waits until the screen pixel `at` is `color`, for up to 20 seconds: a
/// navigation opens a stream and paints when its scene arrives, which takes
/// as long as the machine's load makes it.
async fn until_pixel(page: &Page, at: Point, color: [u8; 3]) {
    let mut last = [0; 3];
    for _ in 0..200 {
        last = pixel(page, at).await;
        if last == color {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for {color:?} at {at:?}, last {last:?}");
}

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

/// How the screenshot `shot` differs from the server's PNG at `png`, laid over
/// the page's background as the screen shows it and with `margin` pixels of
/// that background around it — compared by the engine's own rule, so the test
/// and the constraint can't drift apart. The margin is where a surface that
/// drew more than the drawing would show it.
async fn difference(
    page: &Page,
    address: &str,
    shot: &[u8],
    png: &str,
    margin: usize,
) -> Option<Difference> {
    let background: [f64; 3] = eval(
        page,
        "getComputedStyle(document.body).backgroundColor.match(/[\\d.]+/g).slice(0, 3).map(Number)",
    )
    .await;
    let served = reqwest::get(format!("{address}{png}"))
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    let (drawing, width, height) = render::pixels(&served).unwrap();
    let (width, height) = (width as usize, height as usize);
    let outer = width + 2 * margin;
    let page_color = [
        background[0] as u8,
        background[1] as u8,
        background[2] as u8,
        255,
    ];
    let mut b = page_color.repeat(outer * (height + 2 * margin));
    for y in 0..height {
        for x in 0..width {
            let px = &drawing[4 * (y * width + x)..][..4];
            let alpha = px[3] as f64 / 255.0;
            let to = &mut b[4 * ((y + margin) * outer + x + margin)..][..4];
            for k in 0..3 {
                to[k] = (px[k] as f64 * alpha + background[k] * (1.0 - alpha)).round() as u8;
            }
        }
    }
    let (mut a, _, _) = render::pixels(shot).unwrap();
    for px in a.chunks_mut(4) {
        px[3] = 255;
    }
    render::compare(&a, &b, outer)
}

/// Each example design on the canvas page matches the server's PNG of it
/// within the engine's tolerance: the CPU canvas is that very painting, the
/// GPU canvas another rasterizer of the same display list.
async fn the_examples_match_their_pngs(webgpu: bool) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let address = serve(root, "docs/designs/checkout/checkout.design").await;
    let (mut browser, events, _profile) = chrome().await;
    let mut failures = Vec::new();
    for (theme, name) in ["light", "dark"].into_iter().flat_map(|theme| {
        ["button", "checkout", "confirmation", "details"].map(|name| (theme, name))
    }) {
        let page = browser.new_page("about:blank").await.unwrap();
        // Room for the tallest example at 100%, at one device pixel per pixel.
        page.execute(SetDeviceMetricsOverrideParams::new(1280, 1000, 1.0, false))
            .await
            .unwrap();
        if !webgpu {
            // A browser without WebGPU, from the page's first script on.
            page.execute(AddScriptToEvaluateOnNewDocumentParams::new(
                "delete Navigator.prototype.gpu;",
            ))
            .await
            .unwrap();
        }
        let path = format!("/docs/designs/checkout/{name}.design");
        page.goto(format!("{address}{path}?theme={theme}"))
            .await
            .unwrap();
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
        let [zoom, width, height]: [f64; 3] = eval(
            &page,
            "(() => { const c = document.querySelector('canvas.design');              return [Number(c.dataset.view.split(',')[0]), ...c.dataset.size.split(',').map(Number)]; })()",
        )
        .await;
        assert_eq!(zoom, 1.0, "{name} isn't shown at 100%");
        // Let the GPU's frame reach the screen.
        tokio::time::sleep(Duration::from_millis(300)).await;
        const MARGIN: usize = 24;
        let shot = screenshot(&page, width, height, MARGIN as f64).await;
        let d = difference(
            &page,
            &address,
            &shot,
            &format!("{path}.png?theme={theme}"),
            MARGIN,
        )
        .await;
        eprintln!("{backend} {theme} {name}: {d:?}");
        if !d.is_some_and(|d| d.within_tolerance()) {
            failures.push(format!("{backend} {theme} {name}: {d:?}"));
        }
        page.close().await.ok();
    }
    assert!(failures.is_empty(), "{failures:#?}");
    browser.close().await.ok();
    events.abort();
}

#[tokio::test]
#[ignore]
async fn the_canvas_on_the_gpu_matches_the_png() {
    the_examples_match_their_pngs(true).await;
}

#[tokio::test]
#[ignore]
async fn the_canvas_on_the_cpu_matches_the_png() {
    the_examples_match_their_pngs(false).await;
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
async fn an_interaction_inside_an_instance_from_another_folder_opens_its_own_target() {
    let dir = folder("nested");
    for (name, source) in [
        (
            "lib/tile.design",
            r##"<Frame width={200} height={40} fill="#2563eb" onClick="navigate ./far.design" />"##,
        ),
        (
            "lib/far.design",
            r##"<Frame width={400} height={300} fill="#fef3c7" />"##,
        ),
        (
            "app/home.design",
            r##"import Tile from "../lib/tile.design"
<Frame width={400} height={300} fill="#f8fafc" layout="column" padding={20}>
  <Tile />
</Frame>"##,
        ),
    ] {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }
    let address = serve(&dir, "app/home.design").await;
    let (mut browser, events, _profile) = chrome().await;
    let page = browser
        .new_page(format!("{address}/app/home.design").as_str())
        .await
        .unwrap();
    let painted = "document.querySelector('body > canvas.design')?.dataset.painted === 'true'";
    until(&page, painted).await;

    // The tile's path is beside the tile's file, not beside the screen's.
    let tile = on_screen(&page, "body > canvas.design", 120.0, 40.0).await;
    page.click(tile).await.unwrap();
    until(&page, "location.pathname === '/lib/far.design'").await;
    until(&page, painted).await;
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
    // At its own size: the 400 × 300 screen is never enlarged to the page.
    let [width, height]: [f64; 2] = eval(
        &page,
        "(() => { const b = document.querySelector('canvas.design').getBoundingClientRect(); \
         return [b.width, b.height]; })()",
    )
    .await;
    assert_eq!([width, height], [400.0, 300.0]);

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
    until_pixel(&page, corner, [0xfe, 0xf3, 0xc7]).await;
    assert_eq!(eval::<String>(&page, "location.pathname").await, "/page.md");

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

    // The wheel alone pans — recorded when the next frame paints, which a
    // busy machine can take a while to reach.
    let before: String = view().await;
    page.evaluate(
        "document.querySelector('canvas.design').dispatchEvent(new WheelEvent('wheel', \
         { deltaX: 40, deltaY: 0, bubbles: true, cancelable: true }))",
    )
    .await
    .unwrap();
    until(
        &page,
        &format!("document.querySelector('canvas.design').dataset.view !== '{before}'"),
    )
    .await;
    let panned = on_screen(&page, "canvas.design", 100.0, 100.0).await;
    assert!((panned.x - (after.x - 40.0)).abs() < 0.5);
    browser.close().await.ok();
    events.abort();
}

#[tokio::test]
#[ignore]
async fn an_undone_typo_clears_the_problems() {
    let dir = folder("undo");
    let (address, designs) = serve_designs(&dir, "screen.design").await;
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
    let file = dir.join("screen.design");
    let good = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, good.replace("<Button label", "<Button lable")).unwrap();
    designs.changed(&file);
    until(&page, "!!document.querySelector('.design-problems')").await;
    // Saved back as it was: the scene is the one the canvas already has.
    std::fs::write(&file, &good).unwrap();
    designs.changed(&file);
    until(&page, "!document.querySelector('.design-problems')").await;
    browser.close().await.ok();
    events.abort();
}

#[tokio::test]
#[ignore]
async fn a_deleted_design_comes_back_when_written_again() {
    let dir = folder("deleted");
    std::fs::write(
        dir.join("page.md"),
        "# Page\n\nThe next screen:\n\n![Next](next.design)\n",
    )
    .unwrap();
    let (address, designs) = serve_designs(&dir, "page.md").await;
    let (mut browser, events, _profile) = chrome().await;
    for at in ["next.design", "page.md"] {
        let page = browser.new_page("about:blank").await.unwrap();
        // Count the watch calls the page makes.
        page.execute(AddScriptToEvaluateOnNewDocumentParams::new(
            "window.watchCalls = 0; const f = window.fetch; \
             window.fetch = (url, ...rest) => { if (String(url).includes('/Watch')) window.watchCalls++; return f(url, ...rest); };",
        ))
        .await
        .unwrap();
        page.goto(format!("{address}/{at}")).await.unwrap();
        until(
            &page,
            "document.querySelector('canvas.design')?.dataset.painted === 'true'",
        )
        .await;
        let file = dir.join("next.design");
        let source = std::fs::read_to_string(&file).unwrap();
        std::fs::remove_file(&file).unwrap();
        designs.changed(&file);
        until(&page, "!!document.querySelector('.design-problems')").await;
        let calls: u32 = eval(&page, "window.watchCalls").await;
        // Deleted, it isn't asked for again and again: the one stream stays
        // open for it. Long enough for three reconnects, were it reconnecting.
        tokio::time::sleep(Duration::from_millis(3500)).await;
        let later: u32 = eval(&page, "window.watchCalls").await;
        assert_eq!(later, calls, "{at}: the page kept reopening its stream");
        // The page fetched again — a branch switch touches every file — and
        // the design written back: it draws again, without a reload.
        if at == "page.md" {
            page.evaluate("load(location.href)").await.ok();
        }
        std::fs::write(&file, source).unwrap();
        designs.changed(&file);
        until(&page, "!document.querySelector('.design-problems')").await;
        page.close().await.ok();
    }
    browser.close().await.ok();
    events.abort();
}

#[tokio::test]
#[ignore]
async fn a_canvas_says_so_when_the_engine_does_not_load() {
    let dir = folder("no-engine");
    let address = serve(&dir, "screen.design").await;
    let (mut browser, events, _profile) = chrome().await;
    let page = browser.new_page("about:blank").await.unwrap();
    // A browser that can't compile the module.
    page.execute(AddScriptToEvaluateOnNewDocumentParams::new(
        "const no = () => Promise.reject(new Error('no WebAssembly here')); \
         WebAssembly.instantiate = no; WebAssembly.instantiateStreaming = no;",
    ))
    .await
    .unwrap();
    page.goto(format!("{address}/screen.design")).await.unwrap();
    until(&page, "!!document.querySelector('.design-failed')").await;
    let text: String = eval(
        &page,
        "document.querySelector('.design-failed').textContent",
    )
    .await;
    assert!(text.contains("design engine didn't load"), "{text}");
    assert!(text.contains("no WebAssembly here"), "{text}");
    browser.close().await.ok();
    events.abort();
}

/// The canvas's CSS size and its gaps to the backdrop's content box, left and
/// right: `[width, height, left gap, right gap]`.
const EMBED_BOX: &str = "(() => { const c = document.querySelector('canvas.design'); \
    const s = c.parentElement; const st = getComputedStyle(s); \
    const b = c.getBoundingClientRect(); const o = s.getBoundingClientRect(); \
    return [b.width, b.height, \
      b.left - o.left - parseFloat(st.paddingLeft), \
      o.right - parseFloat(st.paddingRight) - b.right]; })()";

/// Opens `path` in a `width` × `height` window and waits for its design.
async fn embedded(
    browser: &mut Browser,
    address: &str,
    path: &str,
    width: i64,
    height: i64,
) -> Page {
    let page = browser.new_page("about:blank").await.unwrap();
    page.execute(SetDeviceMetricsOverrideParams::new(
        width, height, 1.0, false,
    ))
    .await
    .unwrap();
    page.goto(format!("{address}{path}")).await.unwrap();
    until(
        &page,
        "document.querySelector('canvas.design')?.dataset.painted === 'true'",
    )
    .await;
    page
}

#[tokio::test]
#[ignore]
async fn an_embedded_design_is_its_own_size_centred_and_only_ever_scaled_down() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = folder("own-size");
    std::fs::write(
        dir.join("page.md"),
        "# Page\n\n![The screen](screen.design)\n",
    )
    .unwrap();
    let address = serve(&dir, "page.md").await;
    let (mut browser, events, _profile) = chrome().await;

    // A wide window: 400 × 300, as the design is, centred on its backdrop.
    let page = embedded(&mut browser, &address, "/page.md", 1280, 1000).await;
    let [w, h, left, right]: [f64; 4] = eval(&page, EMBED_BOX).await;
    assert_eq!([w, h], [400.0, 300.0]);
    assert!(
        left > 0.0 && (left - right).abs() <= 1.0,
        "{left} vs {right}"
    );
    assert!(
        eval::<String>(
            &page,
            "document.querySelector('canvas.design').dataset.view"
        )
        .await
        .starts_with("1,")
    );

    // A narrow window: scaled down to the backdrop's width, keeping its shape.
    let page = embedded(&mut browser, &address, "/page.md", 300, 1000).await;
    let [w, h, left, right]: [f64; 4] = eval(&page, EMBED_BOX).await;
    assert!(
        w < 300.0 && left >= 0.0 && right >= -0.5,
        "{w} {left} {right}"
    );
    assert!((h - w * 0.75).abs() <= 1.0, "{h} for {w}");

    // A short window: scaled down until the whole design is in view.
    let page = embedded(&mut browser, &address, "/page.md", 1280, 240).await;
    let [w, h, _, _]: [f64; 4] = eval(&page, EMBED_BOX).await;
    assert!(h <= 240.0 && (h - w * 0.75).abs() <= 1.0, "{w} × {h}");

    // The real example: the 390-wide checkout screen stays 390 wide in a doc.
    let address = serve(root, "docs/boxes/viewer/designs/README.md").await;
    let page = embedded(
        &mut browser,
        &address,
        "/docs/boxes/viewer/designs/README.md",
        1280,
        1000,
    )
    .await;
    let [w, _, left, right]: [f64; 4] = eval(&page, EMBED_BOX).await;
    assert_eq!(w, 390.0);
    assert!((left - right).abs() <= 1.0, "{left} vs {right}");
    browser.close().await.ok();
    events.abort();
}

#[tokio::test]
#[ignore]
async fn the_backdrops_link_opens_the_design_it_shows_on_its_canvas_page() {
    let dir = folder("open-link");
    std::fs::create_dir_all(dir.join("notes")).unwrap();
    std::fs::write(
        dir.join("notes/page.md"),
        "# Page\n\n![The screen](../screen.design)\n",
    )
    .unwrap();
    let address = serve(&dir, "notes/page.md").await;
    let (mut browser, events, _profile) = chrome().await;
    let page = embedded(&mut browser, &address, "/notes/page.md", 1280, 1000).await;
    let link = "document.querySelector('.design-embed a.design-open')";
    let theme: String = eval(&page, "document.documentElement.dataset.theme").await;
    assert_eq!(
        eval::<String>(&page, &format!("{link}.getAttribute('href')")).await,
        format!("/screen.design?theme={theme}")
    );
    // After a navigation inside the embed the link follows the design shown.
    let corner = on_screen(&page, "canvas.design", 300.0, 40.0).await;
    let pay = on_screen(&page, "canvas.design", 120.0, 40.0).await;
    page.click(pay).await.unwrap();
    until_pixel(&page, corner, [0xfe, 0xf3, 0xc7]).await;
    assert_eq!(
        eval::<String>(&page, &format!("{link}.getAttribute('href')")).await,
        format!("/next.design?theme={theme}")
    );
    page.evaluate(format!("{link}.click()")).await.unwrap();
    until(
        &page,
        "location.pathname === '/next.design' && document.body.classList.contains('design-page')",
    )
    .await;
    browser.close().await.ok();
    events.abort();
}

#[tokio::test]
#[ignore]
async fn a_small_design_is_not_enlarged_on_its_canvas_page() {
    let dir = folder("small");
    let address = serve(&dir, "button.design").await;
    let (mut browser, events, _profile) = chrome().await;
    let page = embedded(&mut browser, &address, "/button.design", 1280, 1000).await;
    let view: String = eval(
        &page,
        "document.querySelector('canvas.design').dataset.view",
    )
    .await;
    let [zoom, x, y]: [f64; 3] = view
        .split(',')
        .map(|n| n.parse().unwrap())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    assert_eq!(zoom, 1.0);
    // Centred in the 1280 × 1000 window.
    assert_eq!(
        [x, y],
        [
            ((1280.0 - 200.0) / 2.0_f64).round(),
            ((1000.0 - 40.0) / 2.0_f64).round()
        ]
    );
    browser.close().await.ok();
    events.abort();
}

/// Counts the page's watch calls, and how many are still open: each call's
/// signal, until the page aborts it.
const WATCH_COUNTER: &str = "window.watches = []; const f = window.fetch; \
    window.fetch = (url, options = {}) => { \
      if (String(url).includes('/Watch')) window.watches.push(options.signal); \
      return f(url, options); };";

const OPEN_WATCHES: &str = "window.watches.filter((s) => s && !s.aborted).length";

/// A page embedding eight designs, each its own file.
fn eight(dir: &Path, name: &str) {
    let screen = std::fs::read_to_string(dir.join("screen.design")).unwrap();
    let mut page = String::from("# Eight\n\n");
    for n in 1..=8 {
        std::fs::write(dir.join(format!("s{n}.design")), &screen).unwrap();
        page.push_str(&format!("![Screen {n}](s{n}.design)\n\n"));
    }
    std::fs::write(dir.join(name), page).unwrap();
}

#[tokio::test]
#[ignore]
async fn eight_embedded_designs_paint_through_one_stream_and_the_page_still_loads() {
    let dir = folder("eight");
    eight(&dir, "page.md");
    let address = serve(&dir, "page.md").await;
    let (mut browser, events, _profile) = chrome().await;
    let page = browser.new_page("about:blank").await.unwrap();
    page.execute(AddScriptToEvaluateOnNewDocumentParams::new(WATCH_COUNTER))
        .await
        .unwrap();
    page.goto(format!("{address}/page.md")).await.unwrap();
    until(
        &page,
        "[...document.querySelectorAll('canvas.design')].filter((c) => c.dataset.painted === 'true').length === 8",
    )
    .await;
    assert_eq!(eval::<u32>(&page, OPEN_WATCHES).await, 1);

    // The page can still fetch: an edit swapped in, every canvas kept.
    page.evaluate("document.querySelectorAll('canvas.design').forEach((c) => c.marked = true)")
        .await
        .unwrap();
    let mut edited = std::fs::read_to_string(dir.join("page.md")).unwrap();
    edited.push_str("Edited.\n");
    std::fs::write(dir.join("page.md"), edited).unwrap();
    page.evaluate("load(location.href)").await.unwrap();
    until(
        &page,
        "document.querySelector('article').textContent.includes('Edited.')",
    )
    .await;
    assert_eq!(
        eval::<u32>(
            &page,
            "[...document.querySelectorAll('canvas.design')].filter((c) => c.marked).length"
        )
        .await,
        8
    );
    // The same designs: the stream wasn't reopened.
    assert_eq!(eval::<u32>(&page, "window.watches.length").await, 1);
    browser.close().await.ok();
    events.abort();
}

#[tokio::test]
#[ignore]
async fn a_doc_with_eight_embeds_prints_as_fast_as_one_with_one() {
    let dir = folder("print-eight");
    eight(&dir, "eight.md");
    std::fs::write(
        dir.join("one.md"),
        "# One\n\n![The screen](screen.design)\n",
    )
    .unwrap();
    let time = |name: &'static str| {
        let dir = dir.clone();
        async move {
            let document = Arc::new(Document::load(&dir.join(name), &dir).unwrap());
            let started = std::time::Instant::now();
            crate::pdf::print(document, &dir.join(format!("{name}.pdf")))
                .await
                .unwrap();
            started.elapsed()
        }
    };
    let one = time("one.md").await;
    let eight = time("eight.md").await;
    // Without one stream per page, the eighth design never painted and the
    // print waited out its 20 seconds.
    assert!(
        eight < one + Duration::from_secs(5),
        "eight embeds took {eight:?}, one took {one:?}"
    );
}

#[tokio::test]
#[ignore]
async fn leaving_the_last_design_in_place_closes_the_stream() {
    let dir = folder("leave");
    std::fs::write(
        dir.join("page.md"),
        "# Page\n\n![The screen](screen.design)\n\n[Plain](plain.md)\n",
    )
    .unwrap();
    std::fs::write(dir.join("plain.md"), "# Plain\n\nNo design.\n").unwrap();
    let address = serve(&dir, "page.md").await;
    let (mut browser, events, _profile) = chrome().await;
    let page = browser.new_page("about:blank").await.unwrap();
    page.execute(AddScriptToEvaluateOnNewDocumentParams::new(WATCH_COUNTER))
        .await
        .unwrap();
    page.goto(format!("{address}/page.md")).await.unwrap();
    until(
        &page,
        "document.querySelector('canvas.design')?.dataset.painted === 'true'",
    )
    .await;
    assert_eq!(eval::<u32>(&page, OPEN_WATCHES).await, 1);
    page.evaluate("document.body.marked = true").await.unwrap();
    page.evaluate("[...document.querySelectorAll('article a')].find((a) => a.textContent === 'Plain').click()")
        .await
        .unwrap();
    until(
        &page,
        "document.querySelector('article').textContent.includes('No design.')",
    )
    .await;
    // In place, not a full load, and no watch stream left open.
    assert!(eval::<bool>(&page, "document.body.marked === true").await);
    until(&page, &format!("{OPEN_WATCHES} === 0")).await;
    browser.close().await.ok();
    events.abort();
}

#[tokio::test]
#[ignore]
async fn an_embed_with_problems_survives_a_page_refetch() {
    let dir = folder("embed-problems");
    std::fs::write(
        dir.join("page.md"),
        "# Page\n\n![The screen](screen.design)\n",
    )
    .unwrap();
    let (address, designs) = serve_designs(&dir, "page.md").await;
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
    page.evaluate("document.querySelector('canvas.design').marked = true")
        .await
        .unwrap();
    let screen = std::fs::read_to_string(dir.join("screen.design")).unwrap();
    std::fs::write(
        dir.join("screen.design"),
        screen.replace("<Button label", "<Button lable"),
    )
    .unwrap();
    designs.changed(&dir.join("screen.design"));
    until(
        &page,
        "!!document.querySelector('.design-embed .design-problems')",
    )
    .await;
    page.evaluate("load(location.href)").await.unwrap();
    // Swapped in, and the canvas — with the problems bar before it — kept.
    until(
        &page,
        "document.querySelector('canvas.design')?.marked === true \
         && !!document.querySelector('.design-embed .design-problems')",
    )
    .await;
    browser.close().await.ok();
    events.abort();
}

/// A design that paints nothing over its 120 × 80 box, so the screen shows
/// whatever the canvas puts behind a design there.
const GLASS: &str = r##"<Frame width={120} height={80} fill="#00000000" />"##;

/// Waits until the screen pixel `inside` matches the pixel `outside`, for up
/// to 20 seconds; returns the color.
async fn until_same(page: &Page, inside: Point, outside: Point) -> [u8; 3] {
    let (mut a, mut b) = ([0; 3], [1; 3]);
    for _ in 0..200 {
        a = pixel(page, inside).await;
        b = pixel(page, outside).await;
        if a == b {
            return a;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out: inside {a:?}, outside {b:?}");
}

#[tokio::test]
#[ignore]
async fn the_canvas_page_switches_theme_and_paints_behind_the_design_in_it() {
    let dir = folder("theme-page");
    std::fs::write(dir.join("glass.design"), GLASS).unwrap();
    let address = serve(&dir, "glass.design").await;
    let (mut browser, events, _profile) = chrome().await;
    let page = browser
        .new_page(format!("{address}/glass.design").as_str())
        .await
        .unwrap();
    until(
        &page,
        "document.querySelector('canvas.design')?.dataset.painted === 'true'",
    )
    .await;
    let inside = on_screen(&page, "canvas.design", 60.0, 40.0).await;
    let outside = on_screen(&page, "canvas.design", 60.0, 140.0).await;
    let before: String = eval(&page, "document.documentElement.dataset.theme").await;
    let first = until_same(&page, inside, outside).await;

    page.find_element(".theme-toggle")
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    let after: String = eval(&page, "document.documentElement.dataset.theme").await;
    assert_ne!(before, after);
    let second = until_same(&page, inside, outside).await;
    assert_ne!(first, second, "the background didn't change with the theme");

    // The choice is kept, under the same name the page script uses.
    page.reload().await.unwrap();
    until(
        &page,
        "document.querySelector('canvas.design')?.dataset.painted === 'true'",
    )
    .await;
    assert_eq!(
        eval::<String>(&page, "document.documentElement.dataset.theme").await,
        after
    );
    browser.close().await.ok();
    events.abort();
}

/// A design in red in its light theme and in blue in its dark one.
fn themed(dir: &Path) {
    std::fs::write(
        dir.join("themes.yml"),
        "kind: themes\nlight:\n  bg: \"#ff0000\"\ndark:\n  bg: \"#0000ff\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("tile.design"),
        "<Frame width={120} height={80} fill={theme.bg} />",
    )
    .unwrap();
}

const RED: [u8; 3] = [255, 0, 0];
const BLUE: [u8; 3] = [0, 0, 255];

/// Opens a fresh tab whose remembered theme is `theme`.
async fn tab_in(browser: &mut Browser, theme: &str) -> Page {
    let page = browser.new_page("about:blank").await.unwrap();
    page.execute(SetDeviceMetricsOverrideParams::new(1280, 1000, 1.0, false))
        .await
        .unwrap();
    page.execute(AddScriptToEvaluateOnNewDocumentParams::new(format!(
        "try {{ if (!sessionStorage.getItem('set')) {{ localStorage.setItem('devpage-theme', '{theme}'); sessionStorage.setItem('set', '1'); }} }} catch {{}}"
    )))
    .await
    .unwrap();
    page
}

/// On a dark page a themed embed is drawn dark; its own switch turns only
/// it, design and backdrop, and is kept when the page switches; the Open
/// link carries the theme it shows; and a design that isn't themed has no
/// switch.
#[tokio::test]
#[ignore]
async fn an_embed_follows_its_page_and_its_own_switch_turns_only_it() {
    let dir = folder("theme-own");
    themed(&dir);
    std::fs::write(
        dir.join("page.md"),
        "# Page\n\n![One](tile.design)\n\n![Two](tile.design)\n\n![Plain](screen.design)\n",
    )
    .unwrap();
    let address = serve(&dir, "page.md").await;
    let (mut browser, events, _profile) = chrome().await;
    let page = tab_in(&mut browser, "dark").await;
    page.goto(format!("{address}/page.md")).await.unwrap();
    until(
        &page,
        "[...document.querySelectorAll('canvas.design')].every((c) => c.dataset.painted === 'true')",
    )
    .await;
    let one = on_screen(&page, r#"canvas.design[aria-label="One"]"#, 60.0, 40.0).await;
    let two = on_screen(&page, r#"canvas.design[aria-label="Two"]"#, 60.0, 40.0).await;
    until_pixel(&page, one, BLUE).await;
    until_pixel(&page, two, BLUE).await;
    let shown = "[...document.querySelectorAll('.design-theme')].map((b) => !b.hidden)";
    assert_eq!(eval::<Vec<bool>>(&page, shown).await, [true, true, false]);
    let backdrop = |label: &str| {
        format!("document.querySelector(\"canvas.design[aria-label='{label}']\").parentElement")
    };
    let backdrop_color =
        |label: &str| format!("getComputedStyle({}).backgroundColor", backdrop(label));
    let dark_backdrop: String = eval(&page, &backdrop_color("Two")).await;

    // The first embed's own switch: only it turns light, backdrop and all.
    page.evaluate(format!(
        "{}.querySelector('.design-theme').click()",
        backdrop("One")
    ))
    .await
    .unwrap();
    until_pixel(&page, one, RED).await;
    assert_eq!(pixel(&page, two).await, BLUE);
    assert_eq!(
        eval::<String>(&page, "document.documentElement.dataset.theme").await,
        "dark",
        "the page is as it was"
    );
    assert_ne!(
        eval::<String>(&page, &backdrop_color("One")).await,
        dark_backdrop
    );
    let href = |label: &str| {
        format!(
            "{}.querySelector('a.design-open').getAttribute('href')",
            backdrop(label)
        )
    };
    assert_eq!(
        eval::<String>(&page, &href("One")).await,
        "/tile.design?theme=light"
    );
    assert_eq!(
        eval::<String>(&page, &href("Two")).await,
        "/tile.design?theme=dark"
    );

    // The page switches to light and back: the second embed follows it, the
    // first stays where the reader put it.
    for (theme, two_color) in [("light", RED), ("dark", BLUE)] {
        page.find_element(".theme-toggle")
            .await
            .unwrap()
            .click()
            .await
            .unwrap();
        assert_eq!(
            eval::<String>(&page, "document.documentElement.dataset.theme").await,
            theme
        );
        until_pixel(&page, two, two_color).await;
        assert_eq!(pixel(&page, one).await, RED, "after the page went {theme}");
    }
    browser.close().await.ok();
    events.abort();
}

/// `?theme=` on the canvas page's address shows that theme for the view and
/// leaves the remembered choice alone.
#[tokio::test]
#[ignore]
async fn the_canvas_page_shows_the_theme_its_address_names() {
    let dir = folder("theme-address");
    themed(&dir);
    let address = serve(&dir, "tile.design").await;
    let (mut browser, events, _profile) = chrome().await;
    let page = tab_in(&mut browser, "light").await;
    page.goto(format!("{address}/tile.design?theme=dark"))
        .await
        .unwrap();
    until(
        &page,
        "document.querySelector('canvas.design')?.dataset.painted === 'true'",
    )
    .await;
    assert_eq!(
        eval::<String>(&page, "document.documentElement.dataset.theme").await,
        "dark"
    );
    let inside = on_screen(&page, "canvas.design", 60.0, 40.0).await;
    until_pixel(&page, inside, BLUE).await;
    assert_eq!(
        eval::<String>(&page, "localStorage.getItem('devpage-theme')").await,
        "light",
        "the remembered choice is untouched"
    );
    // Its switch still switches, and is remembered.
    page.find_element(".theme-toggle")
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    until_pixel(&page, inside, RED).await;
    assert_eq!(
        eval::<String>(&page, "localStorage.getItem('devpage-theme')").await,
        "light"
    );
    browser.close().await.ok();
    events.abort();
}

#[tokio::test]
#[ignore]
async fn an_embedded_design_paints_again_when_the_page_switches_theme() {
    let dir = folder("theme-embed");
    std::fs::write(dir.join("glass.design"), GLASS).unwrap();
    std::fs::write(dir.join("page.md"), "# Page\n\n![Glass](glass.design)\n").unwrap();
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
    // Inside the design, and on the backdrop beside it, clear of the shadow the
    // backdrop puts under a design.
    let inside = on_screen(&page, "canvas.design", 60.0, 40.0).await;
    let outside = on_screen(&page, "canvas.design", -120.0, 40.0).await;
    let first = until_same(&page, inside, outside).await;
    let before: String = eval(&page, "document.documentElement.dataset.theme").await;
    page.find_element(".theme-toggle")
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    let after: String = eval(&page, "document.documentElement.dataset.theme").await;
    assert_ne!(before, after);
    let second = until_same(&page, inside, outside).await;
    assert_ne!(first, second, "the backdrop didn't change with the theme");
    browser.close().await.ok();
    events.abort();
}

#[tokio::test]
#[ignore]
async fn a_design_on_a_widget_slide_fits_the_slide_on_screen_and_on_paper() {
    let dir = folder("slide");
    // A tall design on the second slide, inside a project widget — as the
    // showcase deck's slides are: React mounts the widget's content, so the
    // design's canvas is made again after the page script first looks.
    std::fs::write(
        dir.join("tall.design"),
        r##"<Frame width={300} height={1400} fill="#2563eb" />"##,
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("widgets")).unwrap();
    std::fs::write(
        dir.join("widgets/Box.tsx"),
        "export default function Box({ children }) { return <div className=\"box\">{children}</div>; }\n",
    )
    .unwrap();
    std::fs::write(dir.join("one.md"), "# One\n").unwrap();
    std::fs::write(
        dir.join("two.md"),
        "# Two\n\n<Box>\n\n![Tall](tall.design)\n\n</Box>\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("deck.yml"),
        "kind: slides\ntitle: Deck\nwidgets: widgets\nslides: [one.md, two.md]\n",
    )
    .unwrap();
    // A design on a slide that isn't shown waits; shown, it fits the room
    // the slide leaves it, inside the slide's 1080 pixels.
    let address = serve(&dir, "deck.yml").await;
    let (mut browser, events, _profile) = chrome().await;
    let page = browser
        .new_page(format!("{address}/deck.yml#1").as_str())
        .await
        .unwrap();
    until(
        &page,
        "!!document.querySelector('.box canvas.design[data-started]')",
    )
    .await;
    // Changing slides refetches the deck, and React mounts the widget's
    // content again: the canvas is measured in the same step that finds it
    // painted, so a remount between two steps can't leave nothing to measure.
    page.evaluate("location.hash = '#2'").await.unwrap();
    until(
        &page,
        "(() => { const c = document.querySelector('.box canvas.design'); \
         if (c?.dataset.painted !== 'true') return false; const s = c.closest('.slide'); \
         const scale = s.getBoundingClientRect().height / s.offsetHeight; \
         window.measured = [(c.getBoundingClientRect().bottom - s.getBoundingClientRect().top) / scale, s.offsetHeight]; \
         return true; })()",
    )
    .await;
    let [bottom, slide]: [f64; 2] = eval(&page, "window.measured").await;
    assert!(
        bottom <= slide,
        "the design runs past the slide: {bottom} > {slide}"
    );
    browser.close().await.ok();
    events.abort();
    // On paper: printed promptly, the design painted in it.
    let document = Arc::new(Document::load(&dir.join("deck.yml"), &dir).unwrap());
    let pdf = dir.join("deck.pdf");
    let started = std::time::Instant::now();
    crate::pdf::print(document, &pdf).await.unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "printing waited out its cap: {:?}",
        started.elapsed()
    );
    let bytes = std::fs::read(&pdf).unwrap();
    assert!(
        bytes.windows(13).any(|w| w == b"/Subtype /Ima"),
        "no image in the printed deck"
    );
}

#[tokio::test]
#[ignore]
async fn an_embed_in_a_widget_keeps_where_it_navigated_across_a_page_refetch() {
    let dir = folder("widget-refetch");
    std::fs::create_dir_all(dir.join("widgets")).unwrap();
    std::fs::write(
        dir.join("widgets/Box.tsx"),
        "export default function Box({ children }) { return <div className=\"box\">{children}</div>; }\n",
    )
    .unwrap();
    let page_source = |text: &str| {
        format!("# Doc\n\n{text}\n\n<Box>\n\n![The screen](screen.design)\n\n</Box>\n")
    };
    std::fs::write(dir.join("doc.md"), page_source("Before.")).unwrap();
    std::fs::write(
        dir.join("doc.yml"),
        "kind: doc\ntitle: Doc\nwidgets: widgets\nchapters: [doc.md]\n",
    )
    .unwrap();
    let address = serve(&dir, "doc.yml").await;
    let (mut browser, events, _profile) = chrome().await;
    let page = browser
        .new_page(format!("{address}/doc.yml").as_str())
        .await
        .unwrap();
    let painted = "document.querySelector('.box canvas.design')?.dataset.painted === 'true'";
    until(&page, painted).await;
    // Pay navigates inside the embed: the next design's yellow.
    let corner = on_screen(&page, ".box canvas.design", 300.0, 40.0).await;
    let pay = on_screen(&page, ".box canvas.design", 120.0, 40.0).await;
    page.click(pay).await.unwrap();
    until_pixel(&page, corner, [0xfe, 0xf3, 0xc7]).await;

    // The page changes and is fetched again: React makes the widget again,
    // and the embed still shows where it was — with its way back.
    std::fs::write(dir.join("doc.md"), page_source("After.")).unwrap();
    page.evaluate("load(location.href)").await.unwrap();
    until(
        &page,
        "document.querySelector('article')?.textContent.includes('After.')",
    )
    .await;
    until(&page, painted).await;
    let corner = on_screen(&page, ".box canvas.design", 300.0, 40.0).await;
    until_pixel(&page, corner, [0xfe, 0xf3, 0xc7]).await;
    assert_eq!(
        eval::<usize>(
            &page,
            "document.querySelectorAll('.box a.design-open').length"
        )
        .await,
        1,
        "one link to the canvas page"
    );
    // Back, inside the embed: the screen's grey again.
    let back = on_screen(&page, ".box canvas.design", 120.0, 40.0).await;
    page.click(back).await.unwrap();
    until_pixel(&page, corner, [0xf8, 0xfa, 0xfc]).await;
    browser.close().await.ok();
    events.abort();
}

#[tokio::test]
#[ignore]
async fn the_canvas_pages_switch_leaves_the_problems_bar_whole() {
    let dir = folder("corner");
    std::fs::write(
        dir.join("broken.design"),
        r##"<Frame width={200} height={100} fill="#ffffff" lable="typo" />"##,
    )
    .unwrap();
    let address = serve(&dir, "broken.design").await;
    let (mut browser, events, _profile) = chrome().await;
    let page = browser
        .new_page(format!("{address}/broken.design").as_str())
        .await
        .unwrap();
    until(&page, "!!document.querySelector('body > .design-problems')").await;
    for theme in ["light", "dark"] {
        page.evaluate(format!(
            "document.documentElement.dataset.theme = '{theme}'"
        ))
        .await
        .unwrap();
        let overlap: bool = eval(
            &page,
            "(() => { const a = document.querySelector('body > .design-problems').getBoundingClientRect(); \
             const b = document.querySelector('#devpage-corner').getBoundingClientRect(); \
             return a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom; })()",
        )
        .await;
        assert!(!overlap, "the switch covers the problems bar in {theme}");
    }
    browser.close().await.ok();
    events.abort();
}

#[tokio::test]
#[ignore]
async fn an_embed_inside_a_widget_on_a_page_in_a_subfolder_finds_its_design() {
    let dir = folder("widget-subfolder");
    let sub = dir.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    designs(&sub);
    std::fs::write(
        dir.join("devpage.yml"),
        "kind: site\ntitle: Site\nroot: README.md\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("README.md"),
        "# Root\n\n## Topics\n\n- [Sub](sub/page.md)\n",
    )
    .unwrap();
    std::fs::write(
        sub.join("page.md"),
        "# Sub\n\n![Plain](button.design)\n\n<Callout>\n\n![In a callout](button.design)\n\n</Callout>\n\n\
<Tabs>\n<Tab title=\"One\">\n\n![In a tab](button.design)\n\n</Tab>\n</Tabs>\n\n\
<Steps>\n\n1. In a step\n\n   ![In a step](button.design)\n\n2. In a callout in a step\n\n   <Callout>\n\n   ![In a callout in a step](button.design)\n\n   </Callout>\n\n</Steps>\n",
    )
    .unwrap();
    let address = serve(&dir, "devpage.yml").await;
    let (mut browser, events, _profile) = chrome().await;
    let page = browser
        .new_page(format!("{address}/sub/page.md").as_str())
        .await
        .unwrap();
    // Five embeds — plain, in a Callout, in a Tab, in a step, and in a Callout
    // inside a step, which mounts only in the nested pass after Steps draws
    // its items — each paints its design and links to it beside the page, not
    // at the top of the served folder.
    until(
        &page,
        "document.querySelectorAll('canvas.design').length === 5 \
         && [...document.querySelectorAll('canvas.design')].every((c) => c.dataset.painted === 'true')",
    )
    .await;
    let links: Vec<String> = eval(
        &page,
        "[...document.querySelectorAll('a.design-open')].map((a) => new URL(a.href).pathname)",
    )
    .await;
    assert_eq!(links, vec!["/sub/button.design"; 5]);
    assert_eq!(
        eval::<usize>(
            &page,
            "document.querySelectorAll('.design-problems').length"
        )
        .await,
        0,
        "no embed reports a problem"
    );
    browser.close().await.ok();
    events.abort();
}

/// A design on a slide, in a window smaller than the 1920 × 1080 stage the
/// deck scales down: the whole drawing is painted — each corner shows its own
/// color — and a click lands where the reader sees it. Checked on the GPU and
/// on the CPU, at two window sizes.
async fn a_design_on_a_scaled_slide_is_painted_whole(webgpu: bool) {
    let dir = folder(if webgpu {
        "scaled-slide-gpu"
    } else {
        "scaled-slide-cpu"
    });
    // Four corner squares of different colors on white; the bottom-right one
    // navigates to a green screen.
    std::fs::write(
        dir.join("corners.design"),
        r##"<Frame width={800} height={600} fill="#ffffff">
  <Rect x={0} y={0} width={200} height={150} fill="#ff0000" />
  <Rect x={600} y={0} width={200} height={150} fill="#00ff00" />
  <Rect x={0} y={450} width={200} height={150} fill="#0000ff" />
  <Rect x={600} y={450} width={200} height={150} fill="#ffff00" onClick="navigate ./next.design" />
</Frame>"##,
    )
    .unwrap();
    std::fs::write(
        dir.join("next.design"),
        r##"<Frame width={800} height={600} fill="#00ff00" />"##,
    )
    .unwrap();
    std::fs::write(
        dir.join("one.md"),
        "# Corners\n\n![Corners](corners.design)\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("deck.yml"),
        "kind: slides\ntitle: Deck\nslides: [one.md]\n",
    )
    .unwrap();
    let address = serve(&dir, "deck.yml").await;
    let (mut browser, events, _profile) = chrome().await;
    for (width, height) in [(1280, 720), (1600, 900)] {
        let page = browser.new_page("about:blank").await.unwrap();
        page.execute(SetDeviceMetricsOverrideParams::new(
            width, height, 1.0, false,
        ))
        .await
        .unwrap();
        if !webgpu {
            page.execute(AddScriptToEvaluateOnNewDocumentParams::new(
                "delete Navigator.prototype.gpu;",
            ))
            .await
            .unwrap();
        }
        page.goto(format!("{address}/deck.yml#1")).await.unwrap();
        until(
            &page,
            "document.querySelector('canvas.design')?.dataset.painted === 'true'",
        )
        .await;
        // Let the GPU's frame reach the screen.
        tokio::time::sleep(Duration::from_millis(300)).await;
        // The canvas's box on the screen, with the slide's scale applied.
        let [left, top, w, h]: [f64; 4] = eval(
            &page,
            "(() => { const b = document.querySelector('canvas.design').getBoundingClientRect(); \
             return [b.left, b.top, b.width, b.height]; })()",
        )
        .await;
        let at = |fx: f64, fy: f64| Point::new(left + w * fx, top + h * fy);
        for (point, color, corner) in [
            (at(0.1, 0.1), [255, 0, 0], "top left"),
            (at(0.9, 0.1), [0, 255, 0], "top right"),
            (at(0.1, 0.9), [0, 0, 255], "bottom left"),
            (at(0.9, 0.9), [255, 255, 0], "bottom right"),
            (at(0.5, 0.5), [255, 255, 255], "middle"),
        ] {
            let seen = pixel(&page, point).await;
            assert_eq!(
                seen, color,
                "{width}×{height}, webgpu {webgpu}: the {corner} shows {seen:?}"
            );
        }
        // The bottom-right square, where the reader sees it, navigates.
        page.click(at(0.9, 0.9)).await.unwrap();
        until_pixel(&page, at(0.1, 0.1), [0, 255, 0]).await;
        page.close().await.ok();
    }
    browser.close().await.ok();
    events.abort();
}

#[tokio::test]
#[ignore]
async fn a_design_on_a_scaled_slide_is_painted_whole_on_the_gpu() {
    a_design_on_a_scaled_slide_is_painted_whole(true).await;
}

#[tokio::test]
#[ignore]
async fn a_design_on_a_scaled_slide_is_painted_whole_on_the_cpu() {
    a_design_on_a_scaled_slide_is_painted_whole(false).await;
}

/// A PNG of `w` × `h` pixels filled with `fill`, painted by the engine.
fn painted_png(w: u32, h: u32, fill: &str) -> Vec<u8> {
    let files: std::collections::HashMap<String, String> = [(
        "p.design".to_string(),
        format!(r##"<Frame width={{{w}}} height={{{h}}} fill="{fill}" />"##),
    )]
    .into();
    let scene = devpage_design::resolve(&files, "p.design").unwrap();
    devpage_design::png(
        &mut devpage_design::Renderer::new(),
        &scene,
        devpage_design::Theme::Light,
    )
    .unwrap()
}

/// How the GPU canvas of the design `name`, open on `page` at 100%, differs
/// from the server's PNG of it — by the engine's one rule, as
/// `the_examples_match_their_pngs` compares the examples. On a failure the
/// screen is kept in the temporary folder for a look.
async fn gpu_against_png(page: &Page, address: &str, name: &str) -> Result<(), String> {
    gpu_against_png_within(page, address, name, 24).await
}

/// `gpu_against_png`, with `margin` pixels of the page's background around
/// the drawing: none for an embed, whose surroundings are its backdrop.
async fn gpu_against_png_within(
    page: &Page,
    address: &str,
    name: &str,
    margin: usize,
) -> Result<(), String> {
    until(
        page,
        "document.querySelector('canvas.design')?.dataset.painted === 'true'",
    )
    .await;
    let backend: String = eval(
        page,
        "document.querySelector('canvas.design').dataset.backend",
    )
    .await;
    assert_eq!(backend, "gpu");
    let [zoom, width, height]: [f64; 3] = eval(
        page,
        "(() => { const c = document.querySelector('canvas.design'); return [Number(c.dataset.view.split(',')[0]), ...c.dataset.size.split(',').map(Number)]; })()",
    )
    .await;
    assert_eq!(zoom, 1.0, "{name} isn't shown at 100%");
    tokio::time::sleep(Duration::from_millis(300)).await;
    let shot = screenshot(page, width, height, margin as f64).await;
    let d = difference(page, address, &shot, &format!("/{name}.design.png"), margin).await;
    eprintln!("gpu {name}: {d:?}");
    if d.is_some_and(|d| d.within_tolerance()) {
        return Ok(());
    }
    let at = std::env::temp_dir().join(format!("devpage-gpu-{name}.png"));
    std::fs::write(&at, &shot).ok();
    Err(format!("gpu {name}: {d:?}; the screen is {}", at.display()))
}

/// Opens each design in `dir` on its canvas page, on the GPU, and compares
/// it with its PNG.
async fn the_gpu_matches_the_png(dir: &Path, names: &[&str]) {
    let address = serve(dir, &format!("{}.design", names[0])).await;
    let (mut browser, events, _profile) = chrome().await;
    let mut failures = Vec::new();
    for name in names {
        let page = browser.new_page("about:blank").await.unwrap();
        page.execute(SetDeviceMetricsOverrideParams::new(1280, 1000, 1.0, false))
            .await
            .unwrap();
        page.goto(format!("{address}/{name}.design")).await.unwrap();
        if let Err(e) = gpu_against_png(&page, &address, name).await {
            failures.push(e);
        }
        page.close().await.ok();
    }
    assert!(failures.is_empty(), "{failures:#?}");
    browser.close().await.ok();
    events.abort();
}

/// Images past what the GPU's atlas holds draw on the GPU as in the PNG: a
/// file wider than the atlas, resampled once to fit, and twelve distinct
/// 2000-pixel files together — vello leaves out, silently, any image its
/// atlas has no room for, and the canvas shows no hole.
#[tokio::test]
#[ignore]
async fn images_larger_than_the_gpu_atlas_draw_on_the_gpu_as_in_the_png() {
    let dir = folder("big-images");
    // Smooth, so sampling it smaller differs between rasterizers only as an
    // edge does.
    std::fs::write(
        dir.join("wide.png"),
        painted_png(
            9000,
            300,
            "linear-gradient(90deg, #ff0000 0%, #00ff00 25%, #0000ff 50%, #ffff00 75%, #ff00ff 100%)",
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("wide.design"),
        r##"<Frame width={980} height={52} fill="#ffffff" padding={10}><Image src="wide.png" width={960} height={32} /></Frame>"##,
    )
    .unwrap();
    let colors = [
        "#ef4444", "#f97316", "#eab308", "#22c55e", "#06b6d4", "#3b82f6", "#8b5cf6", "#ec4899",
        "#14b8a6", "#84cc16", "#a855f7", "#64748b",
    ];
    let mut rows = [String::new(), String::new()];
    for (i, c) in colors.iter().enumerate() {
        std::fs::write(
            dir.join(format!("big{i}.png")),
            painted_png(
                2000,
                2000,
                &format!("linear-gradient({}deg, {c} 0%, #ffffff 100%)", 30 * i),
            ),
        )
        .unwrap();
        rows[i / 6].push_str(&format!(
            r#"<Image src="big{i}.png" width={{90}} height={{90}} />"#
        ));
    }
    let many = |rows: &[String; 2]| {
        format!(
            r##"<Frame layout="column" gap={{6}} padding={{10}} fill="#ffffff">
  <Frame layout="row" gap={{6}}>{}</Frame>
  <Frame layout="row" gap={{6}}>{}</Frame>
</Frame>"##,
            rows[0], rows[1]
        )
    };
    std::fs::write(dir.join("many.design"), many(&rows)).unwrap();
    the_gpu_matches_the_png(&dir, &["wide", "many"]).await;

    // A save that swaps six of them for six new ones: drawn with an atlas
    // of its own, the new drawing's images don't compete with the last's.
    let (address, designs) = serve_designs(&dir, "many.design").await;
    let (mut browser, events, _profile) = chrome().await;
    let page = browser.new_page("about:blank").await.unwrap();
    page.execute(SetDeviceMetricsOverrideParams::new(1280, 1000, 1.0, false))
        .await
        .unwrap();
    page.goto(format!("{address}/many.design")).await.unwrap();
    gpu_against_png(&page, &address, "many").await.unwrap();
    let mut swapped = String::new();
    for (i, color) in colors.iter().take(6).enumerate() {
        std::fs::write(
            dir.join(format!("new{i}.png")),
            painted_png(
                2000,
                2000,
                &format!(
                    "linear-gradient({}deg, #0f172a 0%, {color} 100%)",
                    45 + 20 * i
                ),
            ),
        )
        .unwrap();
        swapped.push_str(&format!(
            r#"<Image src="new{i}.png" width={{90}} height={{90}} />"#
        ));
    }
    eval::<bool>(
        &page,
        "(document.querySelector('canvas.design').dataset.painted = 'false', true)",
    )
    .await;
    std::fs::write(dir.join("many.design"), many(&[rows[0].clone(), swapped])).unwrap();
    designs.changed(&dir.join("many.design"));
    gpu_against_png(&page, &address, "many").await.unwrap();
    browser.close().await.ok();
    events.abort();
}

/// Shadows drawn in pieces — a box far larger than its blur, round corners
/// as discs' edges, an ellipse in wedges — draw on the GPU as in the PNG:
/// the pieces add up there too, with no seam.
#[tokio::test]
#[ignore]
async fn shadows_in_pieces_draw_on_the_gpu_as_in_the_png() {
    let dir = folder("shadow-pieces");
    std::fs::write(
        dir.join("shadows.design"),
        r##"<Frame width={1100} height={760} fill="#f8fafc" layout="none">
  <Rect x={40} y={40} width={600} height={400} radius={[2, 60, 12, 120]} fill="#ffffff" shadow={{ y: 6, blur: 4, color: "#0f172a66" }} />
  <Ellipse x={700} y={60} width={360} height={200} fill="#ffffff" shadow={{ x: 4, blur: 3, color: "#1e3a8a80" }} />
  <Ellipse x={700} y={320} width={360} height={360} fill="#ffffff" shadow={{ blur: 2, spread: 6, color: "#000000" }} />
  <Rect x={40} y={500} width={600} height={6} fill="#ffffff" shadow={{ blur: 5, color: "#7c3aed" }} />
  <Rect x={40} y={560} width={620} height={160} radius={80} fill="#ffffff" shadow={{ blur: 2, color: "#000000aa" }} />
</Frame>"##,
    )
    .unwrap();
    the_gpu_matches_the_png(&dir, &["shadows"]).await;
}

/// Navigating on one canvas from a design whose images fill the GPU's atlas
/// — four 4096-pixel squares — to another with an image of its own draws
/// the second as its PNG does, on a canvas page and inside an embed alike:
/// the new drawing's images never share an atlas with the last design's.
#[tokio::test]
#[ignore]
async fn a_navigation_from_a_full_atlas_draws_the_next_design_as_its_png() {
    let dir = folder("atlas-navigation");
    for (name, color) in [
        ("red", "#dc0000"),
        ("green", "#00c800"),
        ("blue", "#0000dc"),
        ("yellow", "#dcdc00"),
        ("magenta", "#c800c8"),
    ] {
        std::fs::write(
            dir.join(format!("{name}.png")),
            painted_png(4096, 4096, color),
        )
        .unwrap();
    }
    std::fs::write(
        dir.join("a.design"),
        r##"<Frame width={400} height={100} fill="#ffffff" layout="row">
  <Image src="red.png" width={100} height={100} onClick="navigate ./b.design" />
  <Image src="green.png" width={100} height={100} />
  <Image src="blue.png" width={100} height={100} />
  <Image src="yellow.png" width={100} height={100} />
</Frame>"##,
    )
    .unwrap();
    std::fs::write(
        dir.join("b.design"),
        r##"<Frame width={400} height={300} fill="#ffffff" layout="row">
  <Image src="magenta.png" width={200} height={200} />
</Frame>"##,
    )
    .unwrap();
    std::fs::write(dir.join("page.md"), "# Page\n\n![A](a.design)\n").unwrap();
    let address = serve(&dir, "a.design").await;
    let (mut browser, events, _profile) = chrome().await;
    let mut failures = Vec::new();
    for (start, margin) in ["a.design", "page.md"].into_iter().zip([24, 0]) {
        let page = browser.new_page("about:blank").await.unwrap();
        page.execute(SetDeviceMetricsOverrideParams::new(1280, 1000, 1.0, false))
            .await
            .unwrap();
        page.goto(format!("{address}/{start}")).await.unwrap();
        if let Err(e) = gpu_against_png_within(&page, &address, "a", margin).await {
            failures.push(format!("{start}: {e}"));
        }
        eval::<bool>(
            &page,
            "(document.querySelector('canvas.design').dataset.painted = 'false', true)",
        )
        .await;
        let red = on_screen(&page, "canvas.design", 50.0, 50.0).await;
        page.click(red).await.unwrap();
        until(
            &page,
            "document.querySelector('canvas.design').dataset.size === '400,300'",
        )
        .await;
        if let Err(e) = gpu_against_png_within(&page, &address, "b", margin).await {
            failures.push(format!("{start}: {e}"));
        }
        page.close().await.ok();
    }
    assert!(failures.is_empty(), "{failures:#?}");
    browser.close().await.ok();
    events.abort();
}

#[tokio::test]
#[ignore]
async fn a_swap_keeps_a_design_outside_a_widget_and_stops_watching_a_removed_widgets() {
    let dir = folder("removed-widget");
    std::fs::create_dir_all(dir.join("widgets")).unwrap();
    std::fs::write(
        dir.join("widgets/Box.tsx"),
        "export default function Box({ children }) { return <div className=\"box\">{children}</div>; }\n",
    )
    .unwrap();
    let with_box =
        "# Doc\n\n![The screen](screen.design)\n\n<Box>\n\n![Next](next.design)\n\n</Box>\n";
    std::fs::write(dir.join("doc.md"), with_box).unwrap();
    std::fs::write(
        dir.join("doc.yml"),
        "kind: doc\ntitle: Doc\nwidgets: widgets\nchapters: [doc.md]\n",
    )
    .unwrap();
    let address = serve(&dir, "doc.yml").await;
    let (mut browser, events, _profile) = chrome().await;
    let page = browser.new_page("about:blank").await.unwrap();
    page.execute(AddScriptToEvaluateOnNewDocumentParams::new(WATCH_COUNTER))
        .await
        .unwrap();
    page.goto(format!("{address}/doc.yml")).await.unwrap();
    until(
        &page,
        "[...document.querySelectorAll('canvas.design')].filter((c) => c.dataset.painted === 'true').length === 2",
    )
    .await;
    page.evaluate("window.outside = [...document.querySelectorAll('canvas.design')].find((c) => !c.closest('.box')); window.before = window.watches.length")
        .await
        .unwrap();
    assert!(eval::<bool>(&page, "!!window.outside && !window.outside.closest('.box')").await);

    // The Box goes: the design outside keeps its canvas, and the page's one
    // stream is opened again without the Box's design.
    std::fs::write(
        dir.join("doc.md"),
        "# Doc\n\n![The screen](screen.design)\n\nNo box.\n",
    )
    .unwrap();
    page.evaluate("load(location.href)").await.unwrap();
    until(
        &page,
        "document.querySelector('article').textContent.includes('No box.')",
    )
    .await;
    until(
        &page,
        &format!("window.watches.length === window.before + 1 && {OPEN_WATCHES} === 1"),
    )
    .await;
    assert!(
        eval::<bool>(
            &page,
            "window.outside === document.querySelector('canvas.design') && !document.querySelector('.box')"
        )
        .await
    );
    browser.close().await.ok();
    events.abort();
}
