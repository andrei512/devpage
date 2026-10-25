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
    let router = server::router(document, changes, server::Identity::Local("T".into()));
    tokio::spawn(async move { axum::serve(listener, router).await.ok() });
    format!("http://{address}")
}

/// Serves `dir`, entered at `entry`, and hands back its design store, so a
/// test can tell it about a saved file as the watcher would.
async fn serve_designs(dir: &Path, entry: &str) -> (String, super::Designs) {
    let document = Arc::new(Document::load(&dir.join(entry), dir).unwrap());
    let designs = super::Designs::new(dir.to_path_buf());
    let router = server::router_with_index(
        Arc::new(crate::document::Entry::new(document)),
        Arc::new(crate::index::Index::build(dir)),
        designs.clone(),
        broadcast::channel(16).0,
        server::Identity::Local("T".into()),
    );
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

/// The design's own `width` × `height` pixels on the canvas page, as a PNG.
async fn screenshot(page: &Page, width: f64, height: f64) -> Vec<u8> {
    let at = on_screen(page, "canvas.design", 0.0, 0.0).await;
    page.screenshot(
        CaptureScreenshotParams::builder()
            .clip(Viewport {
                x: at.x,
                y: at.y,
                width,
                height,
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
/// the page's background as the screen shows it — compared by the engine's
/// own rule, so the test and the constraint can't drift apart.
async fn difference(page: &Page, address: &str, shot: &[u8], png: &str) -> Option<Difference> {
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
    let (mut b, width, _) = render::pixels(&served).unwrap();
    for px in b.chunks_mut(4) {
        let alpha = px[3] as f64 / 255.0;
        for k in 0..3 {
            px[k] = (px[k] as f64 * alpha + background[k] * (1.0 - alpha)).round() as u8;
        }
        px[3] = 255;
    }
    let (mut a, _, _) = render::pixels(shot).unwrap();
    for px in a.chunks_mut(4) {
        px[3] = 255;
    }
    render::compare(&a, &b, width as usize)
}

/// Each example design on the canvas page matches the server's PNG of it
/// within the engine's tolerance: the CPU canvas is that very painting, the
/// GPU canvas another rasterizer of the same display list.
async fn the_examples_match_their_pngs(webgpu: bool) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let address = serve(root, "examples/checkout/checkout.design").await;
    let (mut browser, events, _profile) = chrome().await;
    let mut failures = Vec::new();
    for name in ["button", "checkout", "confirmation", "details"] {
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
        let path = format!("/examples/checkout/{name}.design");
        page.goto(format!("{address}{path}")).await.unwrap();
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
        let shot = screenshot(&page, width, height).await;
        let d = difference(&page, &address, &shot, &format!("{path}.png")).await;
        eprintln!("{backend} {name}: {d:?}");
        if !d.is_some_and(|d| d.within_tolerance()) {
            failures.push(format!("{backend} {name}: {d:?}"));
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
async fn a_deleted_design_stops_asking_for_its_stream() {
    let dir = folder("deleted");
    let (address, designs) = serve_designs(&dir, "screen.design").await;
    let (mut browser, events, _profile) = chrome().await;
    let page = browser.new_page("about:blank").await.unwrap();
    // Count the watch calls the page makes.
    page.execute(AddScriptToEvaluateOnNewDocumentParams::new(
        "window.watchCalls = 0; const f = window.fetch; \
         window.fetch = (url, ...rest) => { if (String(url).includes('/Watch')) window.watchCalls++; return f(url, ...rest); };",
    ))
    .await
    .unwrap();
    page.goto(format!("{address}/next.design")).await.unwrap();
    until(
        &page,
        "document.querySelector('canvas.design')?.dataset.painted === 'true'",
    )
    .await;
    std::fs::remove_file(dir.join("next.design")).unwrap();
    designs.changed(&dir.join("next.design"));
    until(&page, "!!document.querySelector('.design-problems')").await;
    let calls: u32 = eval(&page, "window.watchCalls").await;
    // Long enough for three reconnects, were it still reconnecting.
    tokio::time::sleep(Duration::from_millis(3500)).await;
    let later: u32 = eval(&page, "window.watchCalls").await;
    assert_eq!(later, calls, "the canvas kept reopening its stream");
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
