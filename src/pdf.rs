//! `devpage pdf`: Chrome prints the page devpage serves, in print mode, so the
//! PDF is the page the browser shows.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::page::PrintToPdfParams;
use chromiumoxide::fetcher::{
    BrowserFetcher, BrowserFetcherOptions, BrowserHost, BrowserKind, BrowserVersion, BuildInfo,
    Platform, Version,
};
use futures::StreamExt;
use tokio::sync::broadcast;

use crate::document::{Document, Kind, Paper, Pdf};
use crate::server;

/// The version of chrome-headless-shell and Chrome for Testing devpage
/// drives. Pinned, so finding the browser makes no network call: the cached
/// copy is used, and only a missing one is downloaded, as that version's zip.
/// It moves with chromiumoxide, whose DevTools protocol it must speak.
pub(crate) const CHROME_VERSION: (u32, u32, u32, u32) = (156, 0, 8078, 47);

/// Prints `document` to `output`.
pub async fn print(document: Arc<Document>, output: &Path) -> Result<()> {
    let params = match &document.kind {
        Kind::Doc { pdf, .. } => doc_params(pdf),
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
        Kind::Design => bail!("pdf prints a doc or slides, not a design"),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (changes, _) = broadcast::channel(1);
    let url = format!("http://{address}{}?print", document.entry());
    let server = tokio::spawn(async move {
        axum::serve(listener, server::printing(document, changes))
            .await
            .ok();
    });

    let profile = Profile::new();
    let config = profile
        .chrome()
        .await?
        .build()
        .map_err(anyhow::Error::msg)?;
    let (mut browser, mut handler) = Browser::launch(config).await.context("starting Chrome")?;
    let events = tokio::spawn(async move { while handler.next().await.is_some() {} });

    // The page is laid out as paper from the start, not only when Chrome
    // prints it: every slide of a deck is shown in print, so an embedded
    // design on any slide is fitted and painted where it will print.
    let page = browser.new_page("about:blank").await?;
    shot(&page).await?;
    page.emulate_media_type(chromiumoxide::page::MediaTypeParams::Print)
        .await?;
    page.goto(url.as_str()).await?;
    wait_ready(&page, &url).await?;
    let bytes = page.pdf(params).await.context("printing")?;
    std::fs::write(output, bytes).with_context(|| format!("writing {}", output.display()))?;

    browser.close().await.ok();
    browser.wait().await.ok();
    events.abort();
    server.abort();
    Ok(())
}

/// True once the page is ready to print: its diagrams drawn and every
/// embedded design that is laid out — on a slide being shown, or anywhere in
/// print — painted, or settled showing its problems. A design on a hidden
/// slide has no layout and never paints, so it doesn't count.
pub(crate) const READY: &str = "document.documentElement.dataset.ready === 'true' \
    && [...document.querySelectorAll('canvas.design')] \
        .filter((c) => c.checkVisibility()).every((c) => c.dataset.painted)";

/// What a page that isn't ready still waits for, for the log.
const MISSING: &str = "[document.documentElement.dataset.ready === 'true' ? null : 'its diagrams', \
    ...[...document.querySelectorAll('canvas.design')] \
        .filter((c) => c.checkVisibility() && !c.dataset.painted) \
        .map((c) => `the design ${c.getAttribute('src')}`)] \
    .filter(Boolean).join(', ')";

/// Waits up to 20 s for `page`, showing `url`, to be [`READY`]. False when it
/// never was: what it still waited for is logged, and the caller goes on with
/// the page as it is.
pub(crate) async fn wait_ready(page: &chromiumoxide::Page, url: &str) -> Result<bool> {
    for _ in 0..400 {
        if page.evaluate(READY).await?.into_value::<bool>()? {
            return Ok(true);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let missing: String = page.evaluate(MISSING).await?.into_value()?;
    eprintln!("{url} wasn't ready after 20 s, still waiting for {missing}");
    Ok(false)
}

/// The header a page devpage's own Chrome shoots — a slide's PNG, or
/// `devpage pdf` — sends with every request it makes. A slide's PNG asked for
/// with it is never drawn, only answered from the cache, so a slide showing
/// its own PNG can't shoot itself without end.
pub(crate) const SHOT_HEADER: &str = "X-Devpage-Shot";

/// Marks every request `page` makes with [`SHOT_HEADER`].
pub(crate) async fn shot(page: &chromiumoxide::Page) -> Result<()> {
    use chromiumoxide::cdp::browser_protocol::network::{Headers, SetExtraHttpHeadersParams};
    page.execute(SetExtraHttpHeadersParams::new(Headers::new(
        serde_json::json!({ SHOT_HEADER: "1" }),
    )))
    .await?;
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
pub(crate) struct Profile(std::path::PathBuf);

impl Profile {
    pub(crate) fn new() -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static RUNS: AtomicUsize = AtomicUsize::new(0);
        Self(std::env::temp_dir().join(format!(
            "devpage-chrome-{}-{}",
            std::process::id(),
            RUNS.fetch_add(1, Ordering::Relaxed)
        )))
    }

    /// [`headless_shell`] using this profile.
    pub(crate) async fn chrome(&self) -> Result<chromiumoxide::browser::BrowserConfigBuilder> {
        Ok(BrowserConfig::builder()
            .chrome_executable(headless_shell().await?)
            .user_data_dir(&self.0))
    }

    /// [`chrome_for_testing`] using this profile.
    pub(crate) async fn chrome_for_testing(
        &self,
    ) -> Result<chromiumoxide::browser::BrowserConfigBuilder> {
        Ok(BrowserConfig::builder()
            .chrome_executable(chrome_for_testing().await?)
            .user_data_dir(&self.0))
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// chrome-headless-shell, downloaded once into devpage's cache folder: the
/// browser devpage drives for `devpage pdf`, a slide's PNG and every Chrome
/// test. It is a bare program with no app bundle, so nothing devpage launches
/// for them shows in the macOS Dock, and every machine prints with the same
/// browser. Prints running at once share one download.
pub(crate) async fn headless_shell() -> Result<std::path::PathBuf> {
    static SHELL: tokio::sync::OnceCell<std::path::PathBuf> = tokio::sync::OnceCell::const_new();
    SHELL
        .get_or_try_init(|| fetch(BrowserKind::ChromeHeadlessShell, "chrome-headless-shell"))
        .await
        .cloned()
}

/// Chrome for Testing, downloaded once into devpage's cache folder beside the
/// shell: `devpage video` needs it, because the shell can't capture a tab.
pub(crate) async fn chrome_for_testing() -> Result<std::path::PathBuf> {
    static CHROME: tokio::sync::OnceCell<std::path::PathBuf> = tokio::sync::OnceCell::const_new();
    CHROME
        .get_or_try_init(|| fetch(BrowserKind::Chrome, "chrome-for-testing"))
        .await
        .cloned()
}

/// Downloads `kind` into `<devpage's cache folder>/<name>` unless it is
/// there already, and returns the program.
async fn fetch(kind: BrowserKind, name: &str) -> Result<std::path::PathBuf> {
    let home = std::env::var_os("HOME").context("no HOME for devpage's cache folder")?;
    let cache = if cfg!(target_os = "macos") {
        Path::new(&home).join("Library/Caches/devpage").join(name)
    } else {
        Path::new(&home).join(".cache/devpage").join(name)
    };
    install(kind, name, &cache, BrowserHost::current(kind)).await
}

/// Downloads `kind` at [`CHROME_VERSION`] from `host` into `cache` unless it
/// is there already, and returns the program.
async fn install(
    kind: BrowserKind,
    name: &str,
    cache: &Path,
    host: BrowserHost,
) -> Result<std::path::PathBuf> {
    let (major, minor, build, patch) = CHROME_VERSION;
    let version = Version::exact(major, minor, build, patch);
    let Some(platform) = platform() else {
        bail!(
            "{name} {version} has no build for {} on {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
    };
    let url = kind.download_url(platform, &BuildInfo::version(version.to_string()), &host);
    std::fs::create_dir_all(cache)?;
    let fetcher = BrowserFetcher::new(
        BrowserFetcherOptions::builder()
            .with_host(host)
            .with_path(cache)
            .with_kind(kind)
            .with_platform(platform)
            .with_version(BrowserVersion::Version(version))
            .build()
            .map_err(anyhow::Error::msg)?,
    );
    let info = fetcher.fetch().await.with_context(|| {
        format!(
            "downloading {name} {version} for {} from {url} into {}",
            kind.folder(platform),
            cache.display()
        )
    })?;
    Ok(info.executable_path)
}

/// The platform whose build of Chrome this machine runs, if Chrome for
/// Testing has one the fetcher knows. There is none for Linux on Arm.
fn platform() -> Option<Platform> {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some(Platform::Linux)
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some(Platform::Mac)
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some(Platform::MacArm)
    } else if cfg!(all(target_os = "windows", target_arch = "x86")) {
        Some(Platform::Win32)
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some(Platform::Win64)
    } else {
        None
    }
}

/// The print call for a doc: its paper, and its header and footer, whose
/// `{page}` and `{pages}` become Chrome's page-number fields.
fn doc_params(pdf: &Pdf) -> PrintToPdfParams {
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

    /// Where the pinned Chrome's programs sit in a cache folder, on the
    /// platforms the tests run on.
    #[cfg(any(target_os = "macos", all(target_os = "linux", target_arch = "x86_64")))]
    fn pinned_programs() -> [(BrowserKind, &'static str, std::path::PathBuf); 2] {
        let (major, minor, build, patch) = CHROME_VERSION;
        let version = format!("{major}.{minor}.{build}.{patch}");
        let platform = cft_platform();
        let folder = if cfg!(target_os = "linux") {
            "linux"
        } else if cfg!(target_arch = "aarch64") {
            "mac_arm"
        } else {
            "mac"
        };
        let folder = Path::new(&format!("{folder}-{version}")).to_path_buf();
        let chrome = if cfg!(target_os = "linux") {
            Path::new("chrome").to_path_buf()
        } else {
            Path::new("Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing")
                .to_path_buf()
        };
        [
            (
                BrowserKind::ChromeHeadlessShell,
                "chrome-headless-shell",
                folder
                    .join(format!("chrome-headless-shell-{platform}"))
                    .join("chrome-headless-shell"),
            ),
            (
                BrowserKind::Chrome,
                "chrome-for-testing",
                folder.join(format!("chrome-{platform}")).join(chrome),
            ),
        ]
    }

    /// The cached copy is used with no network call: the host is an address
    /// nothing listens on.
    #[cfg(any(target_os = "macos", all(target_os = "linux", target_arch = "x86_64")))]
    #[tokio::test]
    async fn the_pinned_chrome_already_in_the_cache_is_used_as_it_is() {
        let caches = std::env::temp_dir().join(format!("devpage-pinned-{}", std::process::id()));
        for (kind, name, program) in pinned_programs() {
            let cache = caches.join(name);
            let program = cache.join(program);
            std::fs::create_dir_all(program.parent().unwrap()).unwrap();
            std::fs::write(&program, "").unwrap();
            let nowhere = BrowserHost::single("http://127.0.0.1:9");
            assert_eq!(install(kind, name, &cache, nowhere).await.unwrap(), program);
        }
        std::fs::remove_dir_all(&caches).ok();
    }

    /// A failed download says which browser, version, platform and address.
    #[cfg(any(target_os = "macos", all(target_os = "linux", target_arch = "x86_64")))]
    #[tokio::test]
    async fn a_failed_download_names_the_browser_version_platform_and_address() {
        let cache =
            std::env::temp_dir().join(format!("devpage-unreachable-{}", std::process::id()));
        let nowhere = BrowserHost::single("http://127.0.0.1:9");
        let error = install(
            BrowserKind::ChromeHeadlessShell,
            "chrome-headless-shell",
            &cache,
            nowhere,
        )
        .await
        .unwrap_err();
        let (major, minor, build, patch) = CHROME_VERSION;
        let platform = cft_platform();
        assert_eq!(
            error.to_string(),
            format!(
                "downloading chrome-headless-shell {major}.{minor}.{build}.{patch} for {platform} \
                 from http://127.0.0.1:9/chrome-for-testing-public/{major}.{minor}.{build}.{patch}/{platform}/chrome-headless-shell-{platform}.zip \
                 into {}",
                cache.display()
            )
        );
        std::fs::remove_dir_all(&cache).ok();
    }

    /// Chrome for Testing's name for this machine's platform.
    #[cfg(any(target_os = "macos", all(target_os = "linux", target_arch = "x86_64")))]
    fn cft_platform() -> &'static str {
        if cfg!(target_os = "linux") {
            "linux64"
        } else if cfg!(target_arch = "aarch64") {
            "mac-arm64"
        } else {
            "mac-x64"
        }
    }

    /// Both browsers' zips exist at [`CHROME_VERSION`] for every platform.
    /// Needs the network; run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn the_pinned_chrome_can_be_downloaded_for_every_platform() {
        let (major, minor, build, patch) = CHROME_VERSION;
        let version = BuildInfo::version(format!("{major}.{minor}.{build}.{patch}"));
        let client = reqwest::Client::new();
        for kind in [BrowserKind::ChromeHeadlessShell, BrowserKind::Chrome] {
            for &platform in Platform::all() {
                let url = kind.download_url(platform, &version, &BrowserHost::current(kind));
                let response = client.head(&url).send().await.unwrap();
                assert_eq!(response.status(), reqwest::StatusCode::OK, "{url}");
            }
        }
    }

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
        let site = Arc::new(Document::load(&dir, &dir).unwrap());
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
        let document = Arc::new(Document::load(&dir.join("doc.md"), &dir).unwrap());
        let output = dir.join("doc.pdf");
        let started = std::time::Instant::now();
        print(document, &output).await.unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        assert!(!pages(&std::fs::read(&output).unwrap()).is_empty());
    }

    /// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn the_fixture_doc_prints_on_a4_with_its_header_and_footer() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let document =
            Arc::new(Document::load(&root.join("tests/fixtures/sync-design"), root).unwrap());
        let Kind::Doc { pdf, .. } = &document.kind else {
            panic!()
        };
        let params = doc_params(pdf);
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

    /// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn the_talk_prints_one_16_by_9_page_per_slide() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let document = Arc::new(Document::load(&root.join("docs/talks/devpage"), root).unwrap());
        let Kind::Slides { slides, .. } = &document.kind else {
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
        let document = Arc::new(Document::load(&dir.join("docs"), &dir).unwrap());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (changes, _) = broadcast::channel(1);
        let router = server::router(document, changes, Some(server::Identity::Local("T".into())));
        tokio::spawn(async move { axum::serve(listener, router).await.ok() });
        let profile = Profile::new();
        let (mut browser, mut handler) =
            Browser::launch(profile.chrome().await.unwrap().build().unwrap())
                .await
                .unwrap();
        let events = tokio::spawn(async move { while handler.next().await.is_some() {} });
        let page = browser
            .new_page(format!("http://{address}/docs/README.md").as_str())
            .await
            .unwrap();
        let count = |page: chromiumoxide::Page| async move {
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
        events.abort();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn a_deck_shows_one_slide_and_moves_with_the_arrow_keys() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let document = Arc::new(Document::load(&root.join("docs/talks/devpage"), root).unwrap());
        let Kind::Slides { slides, .. } = &document.kind else {
            panic!()
        };
        let last = format!("{0}#{0}", slides.len());
        let url = document.entry();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (changes, _) = broadcast::channel(1);
        let router = server::router(document, changes, Some(server::Identity::Local("T".into())));
        tokio::spawn(async move { axum::serve(listener, router).await.ok() });
        let profile = Profile::new();
        let (mut browser, mut handler) =
            Browser::launch(profile.chrome().await.unwrap().build().unwrap())
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
        assert_eq!(shown(page.clone()).await, last);
        // ⌘← is the browser's, not the deck's.
        page.evaluate("document.dispatchEvent(new KeyboardEvent('keydown', {key: 'ArrowLeft', metaKey: true}))")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(shown(page.clone()).await, last);
        // A live update swaps the deck in place and stays on the slide shown.
        page.evaluate("load(location.href)").await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(shown(page.clone()).await, last);
        browser.close().await.ok();
        events.abort();
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
        let document = Arc::new(Document::load(&dir.join("docs/talk.yml"), &dir).unwrap());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (changes, _) = broadcast::channel(1);
        let router = server::router(document, changes, None);
        tokio::spawn(async move { axum::serve(listener, router).await.ok() });
        let profile = Profile::new();
        let (mut browser, mut handler) =
            Browser::launch(profile.chrome().await.unwrap().build().unwrap())
                .await
                .unwrap();
        let events = tokio::spawn(async move { while handler.next().await.is_some() {} });
        let page = browser
            .new_page(format!("http://{address}/docs/talk.yml").as_str())
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
        events.abort();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn a_docs_outline_moves_above_the_text_on_a_narrow_screen() {
        use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let document =
            Arc::new(Document::load(&root.join("tests/fixtures/sync-design"), root).unwrap());
        let url = document.entry();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (changes, _) = broadcast::channel(1);
        let router = server::router(document, changes, Some(server::Identity::Local("T".into())));
        tokio::spawn(async move { axum::serve(listener, router).await.ok() });
        let profile = Profile::new();
        let (mut browser, mut handler) =
            Browser::launch(profile.chrome().await.unwrap().build().unwrap())
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
        events.abort();
    }

    /// A served folder for the widget tests: `docs/` is a site with the given
    /// README, and `files` are written beside it.
    fn widget_folder(name: &str, readme: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
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
    async fn open(
        dir: &Path,
        path: &str,
    ) -> (
        Browser,
        chromiumoxide::Page,
        tokio::task::JoinHandle<()>,
        Profile,
    ) {
        let (browser, page, events, profile, _) = open_logged(dir, path).await;
        (browser, page, events, profile)
    }

    /// Every request the server answered, as `(path, status)`, in order.
    type Answered = Arc<std::sync::Mutex<Vec<(String, u16)>>>;

    /// [`open`], also keeping every request the server answers.
    async fn open_logged(
        dir: &Path,
        path: &str,
    ) -> (
        Browser,
        chromiumoxide::Page,
        tokio::task::JoinHandle<()>,
        Profile,
        Answered,
    ) {
        let document = Arc::new(Document::load(&dir.join("docs"), dir).unwrap());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (changes, _) = broadcast::channel(1);
        let answered = Answered::default();
        let log = answered.clone();
        let router = server::router(document, changes, Some(server::Identity::Local("T".into())))
            .layer(axum::middleware::from_fn(
                move |request: axum::extract::Request, next: axum::middleware::Next| {
                    let log = log.clone();
                    async move {
                        let path = request.uri().path().to_string();
                        let response = next.run(request).await;
                        log.lock().unwrap().push((path, response.status().as_u16()));
                        response
                    }
                },
            ));
        tokio::spawn(async move { axum::serve(listener, router).await.ok() });
        let profile = Profile::new();
        let (browser, mut handler) =
            Browser::launch(profile.chrome().await.unwrap().build().unwrap())
                .await
                .unwrap();
        let events = tokio::spawn(async move { while handler.next().await.is_some() {} });
        let page = browser
            .new_page(format!("http://{address}{path}").as_str())
            .await
            .unwrap();
        for _ in 0..50 {
            let ready: Option<String> = page
                .evaluate("document.documentElement.dataset.ready ?? null")
                .await
                .ok()
                .and_then(|v| v.into_value().ok());
            if ready.as_deref() == Some("true") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        (browser, page, events, profile, answered)
    }

    /// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn a_cold_load_asks_for_every_module_from_the_head_and_a_plain_page_for_none() {
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
        // A plain page fetches neither pele nor React, nor any widget.
        let (mut browser, page, events, _profile, answered) =
            open_logged(&dir, "/docs/plain.md").await;
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
            !log.iter()
                .any(|(p, _)| p.contains("pele") || p.contains("react") || p.contains("widgets")),
            "{log:?}"
        );
        let address = page.url().await.unwrap().unwrap();
        let address = address.split("/docs/").next().unwrap().to_string();

        // Cold — none of its modules fetched yet: every module the page
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
        for _ in 0..50 {
            if text(&cold, "String(document.documentElement.dataset.ready)").await == "true" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
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
        while let Ok(Some(event)) =
            tokio::time::timeout(Duration::from_millis(200), sent.next()).await
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
            "pele.",
            "Card.tsx",
            "label.ts",
            "Tabs.tsx",
            "Tab.tsx",
            "Diagram.tsx",
            "design.",
            "design-engine.",
        ] {
            assert!(
                modules.iter().any(|(url, _)| url.contains(name)),
                "{name} in {modules:?}"
            );
        }
        let by_script: Vec<_> = modules
            .iter()
            .filter(|(_, initiator)| *initiator == InitiatorType::Script)
            .collect();
        assert!(by_script.is_empty(), "started by script: {by_script:#?}");
        // Each downloaded once: the preload and the import agree.
        let log = answered.lock().unwrap().clone();
        for (url, _) in &modules {
            let path = url[address.len()..].split('?').next().unwrap();
            assert_eq!(
                log.iter().filter(|(p, _)| p == path).count(),
                1,
                "{path} in {log:?}"
            );
        }

        // A page loaded in place brings its preloads into the head.
        assert_eq!(
            text(
                &page,
                "load('/docs/README.md').then(() => [...document.head.querySelectorAll('link[rel=modulepreload]')].some((l) => l.href.includes('/pele.')) && !!document.querySelector('.diagram svg')).then(String)"
            )
            .await,
            "true"
        );
        browser.close().await.ok();
        events.abort();
    }

    async fn text(page: &chromiumoxide::Page, script: &str) -> String {
        page.evaluate(script).await.unwrap().into_value().unwrap()
    }

    const NAVBAR_SITE: &str = "kind: site\ntitle: Garden\nroot: README.md\nnavbar:\n  Home (house): README.md\n  Guide (book-open): guide.md\n  More (layout-grid):\n    Plan (file-text): plan.md\n    Web: https://example.com/garden\n  (github): https://github.com/someone/garden\n";

    fn navbar_folder(name: &str) -> std::path::PathBuf {
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
        let (mut browser, page, events, _profile) = open(&dir, "/docs/README.md").await;
        // Loaded, 22px, before the title and centred on it, and the tab's icon.
        let shown = "(() => { const i = document.querySelector('.navbar-title .logo'), t = document.querySelector('.navbar-title'); const a = i.getBoundingClientRect(), b = t.getBoundingClientRect(); return [i.complete && i.naturalWidth > 0, a.width, a.height, i === t.firstElementChild, Math.abs((a.top + a.bottom) / 2 - (b.top + b.bottom) / 2) < 1.5, document.querySelector('link[rel=icon]').getAttribute('href')].join(' '); })()";
        assert_eq!(
            text(&page, shown).await,
            "true 22 22 true true /docs/logo.svg"
        );
        browser.close().await.ok();
        events.abort();
    }

    /// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn the_navbar_opens_its_dropdown_and_follows_the_page_shown() {
        let dir = navbar_folder("navbar-menu");
        let (mut browser, page, events, _profile) = open(&dir, "/docs/README.md").await;
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
        events.abort();
    }

    /// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn the_theme_toggle_switches_every_token_and_remembers_the_choice() {
        let dir = navbar_folder("navbar-theme");
        let (mut browser, page, events, _profile) = open(&dir, "/docs/README.md").await;
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
        events.abort();
    }

    /// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn on_a_narrow_screen_the_navbar_folds_into_a_menu() {
        use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
        let dir = navbar_folder("navbar-narrow");
        let (mut browser, page, events, _profile) = open(&dir, "/docs/README.md").await;
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
        events.abort();
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
        let (mut browser, page, events, _profile) = open(&dir, "/docs/README.md").await;
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
            v["cards"],
            "Start→:README.md:true:begin <em>here</em>,Plain→:README.md:false:no icon|2",
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
        events.abort();
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
        let (mut browser, page, events, _profile) = open(&dir, "/docs/design/devpage.yml").await;
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
        events.abort();
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
        let (mut browser, page, events, _profile) = open(&dir, "/docs/README.md").await;
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
        events.abort();
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
        let (mut browser, page, events, _profile, answered) =
            open_logged(&dir, "/docs/README.md").await;
        assert_eq!(
            text(&page, "document.querySelector('.label').textContent").await,
            "w"
        );
        let first = answered.lock().unwrap().len();
        page.reload().await.unwrap();
        for _ in 0..50 {
            let ready: Option<String> = page
                .evaluate("document.documentElement.dataset.ready ?? null")
                .await
                .ok()
                .and_then(|v| v.into_value().ok());
            if ready.as_deref() == Some("true") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
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
        events.abort();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn a_change_that_leaves_the_page_as_it_was_leaves_the_dom_untouched() {
        let dir = widget_folder("untouched", "# W\n\n<Callout>c</Callout>\n", &[]);
        let (mut browser, page, events, _profile) = open(&dir, "/docs/README.md").await;
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
        events.abort();
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
        let (mut browser, page, events, _profile) = open(&dir, "/docs/README.md").await;
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
        events.abort();
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
        let (mut browser, page, events, _profile) = open(&dir, "/docs/README.md").await;
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
        events.abort();
        std::fs::remove_dir_all(&dir).ok();
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
                    "# One\n\n<Tabs>\n<Tab title=\"First\">\n\nshown on paper\n\n</Tab>\n<Tab title=\"Second\">\n\nonly on screen\n\n</Tab>\n</Tabs>\n\n<Terminal>\n\n```\n$ echo a long line\n```\n\n</Terminal>\n",
                ),
            ],
        );
        let (mut browser, page, events, _profile) =
            open(&dir, "/docs/design/devpage.yml?print").await;
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
        browser.close().await.ok();
        events.abort();
        let document = Arc::new(Document::load(&dir.join("docs/design"), &dir).unwrap());
        let out = dir.join("d.pdf");
        print(document, &out).await.unwrap();
        assert!(!pages(&std::fs::read(&out).unwrap()).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
