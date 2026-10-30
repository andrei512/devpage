//! Chrome, as devpage drives it: one [`Session`] — a Chrome with a profile
//! folder of its own, and the router it loads pages from, served on loopback
//! — launched in one of two [`Profile`]s. `devpage pdf` and a slide's PNG use
//! the shot profile, `devpage video` the recording one. A tab is [`opened`]
//! once its page is ready, and then evaluated in, waited on, screenshot,
//! printed and closed.
//!
//! The browsers themselves are pinned and downloaded once into devpage's
//! cache folder, never a Chrome installed on the machine.
//!
//! [`opened`]: Session::open

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use axum::Router;
use chromiumoxide::Page;
use chromiumoxide::browser::{Browser, BrowserConfig, BrowserConfigBuilder};
use chromiumoxide::cdp::browser_protocol::browser::{
    Bounds, GetWindowBoundsParams, GetWindowForTargetParams, SetWindowBoundsParams,
};
use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
use chromiumoxide::cdp::browser_protocol::input::{DispatchKeyEventParams, DispatchKeyEventType};
use chromiumoxide::cdp::browser_protocol::network::{Headers, SetExtraHttpHeadersParams};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotParams;
use chromiumoxide::cdp::js_protocol::runtime::EvaluateParams;
use chromiumoxide::fetcher::{
    BrowserFetcher, BrowserFetcherOptions, BrowserHost, BrowserKind, BrowserVersion, BuildInfo,
    Platform, Version,
};
use futures::StreamExt;
use serde::de::DeserializeOwned;
use tokio::task::JoinHandle;

pub(crate) use chromiumoxide::cdp::browser_protocol::page::PrintToPdfParams;

/// The version of chrome-headless-shell and Chrome for Testing devpage
/// drives. Pinned, so finding the browser makes no network call: the cached
/// copy is used, and only a missing one is downloaded, as that version's zip.
/// It moves with chromiumoxide, whose DevTools protocol it must speak.
pub(crate) const CHROME_VERSION: (u32, u32, u32, u32) = (156, 0, 8078, 47);

/// The header a tab opened to be shot — a slide's PNG, or `devpage pdf` —
/// sends with every request it makes. A slide's PNG asked for with it is
/// never drawn, only answered from the cache, so a slide showing its own PNG
/// can't shoot itself without end.
pub(crate) const SHOT_HEADER: &str = "X-Devpage-Shot";

/// True once the page is ready: its widgets mounted and every embedded design
/// that is laid out — on a slide being shown, or anywhere in print — painted,
/// or settled showing its problems. A design on a hidden slide has no layout
/// and never paints, so it doesn't count.
const READY: &str = "document.documentElement.dataset.ready === 'true' \
    && [...document.querySelectorAll('canvas.design')] \
        .filter((c) => c.checkVisibility()).every((c) => c.dataset.painted)";

/// What a page that isn't ready still waits for, for the log.
const MISSING: &str = "[document.documentElement.dataset.ready === 'true' ? null : 'its widgets', \
    ...[...document.querySelectorAll('canvas.design')] \
        .filter((c) => c.checkVisibility() && !c.dataset.painted) \
        .map((c) => `the design ${c.getAttribute('src')}`)] \
    .filter(Boolean).join(', ')";

/// How long a tab waits for its page to be [`READY`] before it is used as it
/// is.
const READY_WITHIN: Duration = Duration::from_secs(20);

/// How often [`Tab::until`] asks again.
const POLL: Duration = Duration::from_millis(50);

/// How devpage launches Chrome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Profile {
    /// chrome-headless-shell with its GPU on, so a screenshot holds an
    /// embedded design's WebGPU canvas as the reader sees it rather than
    /// black: for `devpage pdf`, a slide's PNG and the Chrome tests.
    Shot,
    /// Chrome for Testing, headless but configured as headed — chromiumoxide's
    /// own headless flags add `--mute-audio` — capturing its own tab without
    /// asking, playing audio without a gesture, its window sized so the page
    /// is 1920 × 1080 below the "sharing this tab" bar: for `devpage video`,
    /// since the shell can't capture a tab.
    Recording,
}

impl Profile {
    /// The browser this profile launches, downloaded the first time.
    async fn program(self) -> Result<PathBuf> {
        match self {
            Profile::Shot => headless_shell().await,
            Profile::Recording => chrome_for_testing().await,
        }
    }

    /// Chrome launched from `program` with its profile folder at `folder`,
    /// its sandbox as `sandbox` — `DEVPAGE_CHROME_SANDBOX` — says.
    fn config(
        self,
        program: &Path,
        folder: &Path,
        sandbox: Option<&str>,
    ) -> Result<BrowserConfigBuilder> {
        let chrome = BrowserConfig::builder()
            .chrome_executable(program)
            .user_data_dir(folder);
        let chrome = match self {
            // chrome-headless-shell keeps its GPU process off unless
            // `enable-gpu` is passed. chromiumoxide adds the `--` itself.
            Profile::Shot => chrome.arg("enable-unsafe-webgpu").arg("enable-gpu"),
            Profile::Recording => chrome
                .viewport(None)
                .disable_default_args()
                .with_head()
                .arg(("headless", "new"))
                .arg("hide-scrollbars")
                .arg("auto-accept-this-tab-capture")
                .arg(("autoplay-policy", "no-user-gesture-required"))
                .window_size(1920, 1167),
        };
        sandboxed(chrome, sandbox)
    }
}

/// Chrome with its sandbox, unless `DEVPAGE_CHROME_SANDBOX` is `off`: in a
/// container, which has no sandbox for Chrome to use and is the boundary
/// itself.
fn sandboxed(chrome: BrowserConfigBuilder, setting: Option<&str>) -> Result<BrowserConfigBuilder> {
    match setting {
        None | Some("") | Some("on") => Ok(chrome),
        Some("off") => Ok(chrome.no_sandbox()),
        Some(other) => bail!("DEVPAGE_CHROME_SANDBOX is {other:?}, not on or off"),
    }
}

/// A running Chrome: the browser, the loop that carries its DevTools events,
/// and its profile folder. Dropped, the browser's process is killed and the
/// folder deleted.
struct Launched {
    browser: Browser,
    events: JoinHandle<()>,
    _folder: Folder,
}

impl Launched {
    /// Chrome in `profile`, with a profile folder of its own, so several can
    /// run at once without fighting over one profile's lock.
    async fn new(
        profile: Profile,
        adjust: impl FnOnce(BrowserConfigBuilder) -> BrowserConfigBuilder,
    ) -> Result<Self> {
        let folder = Folder::new();
        let sandbox = std::env::var("DEVPAGE_CHROME_SANDBOX").ok();
        let config =
            adjust(profile.config(&profile.program().await?, &folder.0, sandbox.as_deref())?)
                .build()
                .map_err(anyhow::Error::msg)?;
        let (browser, mut handler) = Browser::launch(config).await.context(match profile {
            Profile::Shot => "starting Chrome",
            Profile::Recording => "starting Chrome for Testing",
        })?;
        let events = tokio::spawn(async move { while handler.next().await.is_some() {} });
        Ok(Self {
            browser,
            events,
            _folder: folder,
        })
    }
}

impl Drop for Launched {
    fn drop(&mut self) {
        self.events.abort();
        // Chrome is killed and waited for here, before its profile folder is
        // deleted: on Windows a running Chrome holds the folder's files open,
        // and the delete would fail.
        if let Some(child) = self.browser.get_mut_child() {
            let child = child.as_mut_inner();
            child.start_kill().ok();
            for _ in 0..100 {
                if !matches!(child.try_wait(), Ok(None)) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
}

/// One Chrome, and the router its tabs load pages from, served on a free
/// port on `127.0.0.1`. Dropped, Chrome is killed and the router stops.
pub(crate) struct Session {
    chrome: Launched,
    server: JoinHandle<()>,
    address: SocketAddr,
    profile: Profile,
}

/// How a tab opens its page.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Open {
    /// Every request the tab makes carries [`SHOT_HEADER`]: it is being shot,
    /// so a slide's PNG it shows is never drawn inside it.
    pub shot: bool,
    /// The screen the page is laid out on, in CSS pixels, instead of the
    /// profile's own.
    pub screen: Option<(u32, u32)>,
    /// Laid out as paper from the start — the print media type — not only
    /// when Chrome prints it.
    pub paper: bool,
}

impl Session {
    /// Serves `router` and launches Chrome in `profile` to load from it.
    pub(crate) async fn start(profile: Profile, router: Router) -> Result<Self> {
        let (address, server) = serve(router).await?;
        match Launched::new(profile, |chrome| chrome).await {
            Ok(chrome) => Ok(Self {
                chrome,
                server,
                address,
                profile,
            }),
            Err(error) => {
                server.abort();
                Err(error)
            }
        }
    }

    /// Whether Chrome is still running.
    pub(crate) fn running(&self) -> bool {
        !self.chrome.events.is_finished()
    }

    /// `path` — with its query and fragment — opened in a new tab as `open`
    /// says, once its page is [`READY`]. A page still not ready after 20 s
    /// is used as it is: the log says what it waited for, and the tab says
    /// it wasn't.
    pub(crate) async fn open(&self, path: &str, open: Open) -> Result<Tab> {
        let page = self.chrome.browser.new_page("about:blank").await?;
        let url = format!("http://{}{path}", self.address);
        let opened = async {
            if open.shot {
                page.execute(SetExtraHttpHeadersParams::new(Headers::new(
                    serde_json::json!({ SHOT_HEADER: "1" }),
                )))
                .await?;
            }
            if let Some((width, height)) = open.screen {
                page.execute(SetDeviceMetricsOverrideParams::new(
                    i64::from(width),
                    i64::from(height),
                    1.0,
                    false,
                ))
                .await?;
            }
            if open.paper {
                page.emulate_media_type(chromiumoxide::page::MediaTypeParams::Print)
                    .await?;
            }
            page.goto(url.as_str()).await?;
            anyhow::Ok(())
        }
        .await;
        let mut tab = Tab { page, ready: false };
        if let Err(error) = opened {
            tab.close().await;
            return Err(error);
        }
        tab.ready = tab.until(READY, READY_WITHIN).await?;
        if !tab.ready {
            let missing: String = tab.eval(MISSING).await?;
            eprintln!(
                "{url} wasn't ready after {} s, still waiting for {missing}",
                READY_WITHIN.as_secs()
            );
        }
        Ok(tab)
    }

    /// Closes Chrome — the shot profile's asked to close, the recording
    /// profile's killed, as `devpage video` ends however its tab was left —
    /// and stops the router.
    pub(crate) async fn close(mut self) {
        let browser = &mut self.chrome.browser;
        match self.profile {
            Profile::Shot => {
                browser.close().await.ok();
            }
            Profile::Recording => {
                browser.kill().await;
            }
        }
        browser.wait().await.ok();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.server.abort();
    }
}

/// A tab of a [`Session`], its page opened.
pub(crate) struct Tab {
    page: Page,
    ready: bool,
}

impl Tab {
    /// False when the page was never [`READY`] and is shown as it was.
    pub(crate) fn ready(&self) -> bool {
        self.ready
    }

    /// Evaluates `expression` in the page as a user gesture, awaiting a
    /// promise, and reads its value; `()` for none.
    pub(crate) async fn eval<T: DeserializeOwned>(&self, expression: &str) -> Result<T> {
        let params = EvaluateParams::builder()
            .expression(expression)
            .user_gesture(true)
            .await_promise(true)
            .return_by_value(true)
            .build()
            .map_err(anyhow::Error::msg)?;
        let result = self.page.evaluate_expression(params).await?;
        match result.value() {
            Some(_) => Ok(result.into_value()?),
            None => Ok(serde_json::from_value(serde_json::Value::Null)?),
        }
    }

    /// Waits until `expression` is true, for up to `within`; false when it
    /// never was.
    pub(crate) async fn until(&self, expression: &str, within: Duration) -> Result<bool> {
        let started = tokio::time::Instant::now();
        loop {
            if self.eval::<bool>(expression).await? {
                return Ok(true);
            }
            if started.elapsed() >= within {
                return Ok(false);
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// The page as it is shown, as a PNG.
    pub(crate) async fn screenshot(&self) -> Result<Vec<u8>> {
        self.page
            .screenshot(CaptureScreenshotParams::builder().build())
            .await
            .context("taking the screenshot")
    }

    /// The page printed, as `params` say, to a PDF's bytes.
    pub(crate) async fn print(&self, params: PrintToPdfParams) -> Result<Vec<u8>> {
        self.page.pdf(params).await.context("printing")
    }

    /// Presses `key` — a DOM key such as `ArrowRight`, with its Windows key
    /// code — on the page.
    pub(crate) async fn press(&self, key: &str, code: i64) -> Result<()> {
        for kind in [DispatchKeyEventType::KeyDown, DispatchKeyEventType::KeyUp] {
            self.page
                .execute(
                    DispatchKeyEventParams::builder()
                        .r#type(kind)
                        .key(key)
                        .code(key)
                        .windows_virtual_key_code(code)
                        .build()
                        .map_err(anyhow::Error::msg)?,
                )
                .await?;
        }
        Ok(())
    }

    /// Grows the tab's window by `width` × `height` pixels — shrinks it,
    /// for negative ones.
    pub(crate) async fn grow_window(&self, width: i64, height: i64) -> Result<()> {
        let window = self
            .page
            .execute(GetWindowForTargetParams {
                target_id: Some(self.page.target_id().clone()),
            })
            .await?
            .result
            .window_id;
        let bounds = self
            .page
            .execute(GetWindowBoundsParams::new(window))
            .await?
            .result
            .bounds;
        self.page
            .execute(SetWindowBoundsParams::new(
                window,
                Bounds {
                    width: bounds.width.map(|now| now + width),
                    height: bounds.height.map(|now| now + height),
                    ..Default::default()
                },
            ))
            .await?;
        Ok(())
    }

    /// Closes the tab.
    pub(crate) async fn close(self) {
        self.page.close().await.ok();
    }
}

/// `router` served on a free loopback port, for a Chrome to load from: its
/// address, and the task serving it.
async fn serve(router: Router) -> Result<(SocketAddr, JoinHandle<()>)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.ok();
    });
    Ok((address, server))
}

/// A Chrome profile folder of its own for one Chrome, in the system's
/// temporary folder. It is deleted when Chrome is done with it.
struct Folder(PathBuf);

impl Folder {
    fn new() -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static RUNS: AtomicUsize = AtomicUsize::new(0);
        Self(std::env::temp_dir().join(format!(
            "devpage-chrome-{}-{}",
            std::process::id(),
            RUNS.fetch_add(1, Ordering::Relaxed)
        )))
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        // Chrome's helper processes outlive it by a moment, and on Windows a
        // file one still holds open can't be deleted: a few tries.
        for _ in 0..20 {
            if std::fs::remove_dir_all(&self.0).is_ok() || !self.0.exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

/// chrome-headless-shell, downloaded once into devpage's cache folder: the
/// browser of the shot profile. It is a bare program with no app bundle, so
/// nothing devpage launches with it shows in the macOS Dock, and every
/// machine prints with the same browser. Launches at once share one
/// download.
async fn headless_shell() -> Result<PathBuf> {
    static SHELL: tokio::sync::OnceCell<PathBuf> = tokio::sync::OnceCell::const_new();
    SHELL
        .get_or_try_init(|| fetch(BrowserKind::ChromeHeadlessShell, "chrome-headless-shell"))
        .await
        .cloned()
}

/// Chrome for Testing, downloaded once into devpage's cache folder beside the
/// shell: the browser of the recording profile.
async fn chrome_for_testing() -> Result<PathBuf> {
    static CHROME: tokio::sync::OnceCell<PathBuf> = tokio::sync::OnceCell::const_new();
    CHROME
        .get_or_try_init(|| fetch(BrowserKind::Chrome, "chrome-for-testing"))
        .await
        .cloned()
}

/// Downloads `kind` into `<devpage's cache folder>/<name>` unless it is
/// there already, and returns the program.
async fn fetch(kind: BrowserKind, name: &str) -> Result<PathBuf> {
    let cache = cache_folder()?.join(name);
    install(kind, name, &cache, BrowserHost::current(kind)).await
}

/// devpage's cache folder: `~/Library/Caches/devpage` on macOS,
/// `%LOCALAPPDATA%\devpage` on Windows, `~/.cache/devpage` elsewhere.
fn cache_folder() -> Result<PathBuf> {
    if cfg!(windows) {
        let local = std::env::var_os("LOCALAPPDATA")
            .context("no LOCALAPPDATA for devpage's cache folder")?;
        return Ok(PathBuf::from(local).join("devpage"));
    }
    let home = std::env::home_dir().context("no home folder for devpage's cache folder")?;
    Ok(if cfg!(target_os = "macos") {
        home.join("Library/Caches/devpage")
    } else {
        home.join(".cache/devpage")
    })
}

/// Downloads `kind` at [`CHROME_VERSION`] from `host` into `cache` unless it
/// is there already, and returns the program.
async fn install(
    kind: BrowserKind,
    name: &str,
    cache: &Path,
    host: BrowserHost,
) -> Result<PathBuf> {
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

/// What the Chrome tests drive the browser with: one Chrome for the whole
/// run, launched in the shot profile as devpage launches it, a browser of its
/// own for each test inside it, the router a test's pages load from served on
/// loopback, and the waits a test needs on a page.
#[cfg(test)]
pub(crate) mod testing {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, OnceLock};

    use axum::Router;
    use chromiumoxide::Page;
    use chromiumoxide::cdp::browser_protocol::browser::BrowserContextId;
    use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
    use chromiumoxide::cdp::browser_protocol::target::{
        CreateBrowserContextParams, CreateTargetParams,
    };
    use serde::de::DeserializeOwned;

    use super::{BrowserConfigBuilder, Launched, Profile, READY_WITHIN};

    /// The one Chrome every test's [`Browser`] lives in, and the runtime
    /// that carries its DevTools events: a runtime of its own, since each
    /// test's runtime ends with the test.
    struct Shared {
        chrome: Launched,
        runtime: tokio::runtime::Handle,
    }

    static SHARED: tokio::sync::OnceCell<Shared> = tokio::sync::OnceCell::const_new();

    /// The run's Chrome, launched by the first test that asks for it. Its
    /// runtime's thread blocks forever, and so keeps it running until the run
    /// exits — when Chrome is killed and its profile folder deleted, as
    /// nothing in a `static` is ever dropped.
    async fn shared() -> &'static Shared {
        SHARED
            .get_or_init(|| async {
                let (sent, launched) = tokio::sync::oneshot::channel();
                std::thread::spawn(move || {
                    let runtime = tokio::runtime::Builder::new_multi_thread()
                        .worker_threads(2)
                        .enable_all()
                        .build()
                        .unwrap();
                    let chrome = runtime.block_on(Launched::new(Profile::Shot, software_gpu));
                    let running = chrome.is_ok();
                    sent.send(chrome.map(|chrome| Shared {
                        chrome,
                        runtime: runtime.handle().clone(),
                    }))
                    .ok();
                    if running {
                        runtime.block_on(std::future::pending::<()>());
                    }
                });
                let mut shared = launched.await.unwrap().unwrap();
                killed_at_exit(&mut shared.chrome);
                shared
            })
            .await
    }

    /// What the run kills and deletes as it exits: the shared Chrome's
    /// process and its profile folder.
    #[cfg(unix)]
    static AT_EXIT: OnceLock<(u32, PathBuf)> = OnceLock::new();

    #[cfg(unix)]
    fn killed_at_exit(chrome: &mut Launched) {
        extern "C" fn kill() {
            if let Some((process, folder)) = AT_EXIT.get() {
                // SAFETY: a signal sent to a process id; nothing is shared.
                unsafe { libc::kill(*process as libc::pid_t, libc::SIGKILL) };
                std::fs::remove_dir_all(folder).ok();
            }
        }
        let process = chrome
            .browser
            .get_mut_child()
            .and_then(|child| child.as_mut_inner().id());
        if let Some(process) = process
            && AT_EXIT.set((process, chrome._folder.0.clone())).is_ok()
        {
            // SAFETY: registers a plain function, which runs once at exit.
            unsafe { libc::atexit(kill) };
        }
    }

    #[cfg(not(unix))]
    fn killed_at_exit(_: &mut Launched) {}

    /// A test's browser: a browser context of the run's one Chrome, with its
    /// own storage, cache, cookies and connections, so tests share nothing
    /// but the process and run side by side. Like any browser it keeps at
    /// most six connections to one server, and a page devpage serves holds up
    /// to two open — its event stream and its watch stream — so more than
    /// three pages open at once against one server take a browser each.
    /// Closed or dropped, its pages close and its storage goes.
    pub(crate) struct Browser {
        context: Option<BrowserContextId>,
        screen: Option<(u32, u32)>,
    }

    /// A browser of the test's own, its pages laid out on chromiumoxide's
    /// 800 × 600 screen.
    pub(crate) async fn browser() -> Browser {
        browser_with(None).await
    }

    /// A browser of the test's own, its pages laid out on a `width` ×
    /// `height` screen.
    pub(crate) async fn browser_on(width: u32, height: u32) -> Browser {
        browser_with(Some((width, height))).await
    }

    async fn browser_with(screen: Option<(u32, u32)>) -> Browser {
        let context = shared()
            .await
            .chrome
            .browser
            .create_browser_context(CreateBrowserContextParams::default())
            .await
            .unwrap();
        Browser {
            context: Some(context),
            screen,
        }
    }

    /// The run's Chrome on SwiftShader's WebGPU adapter when
    /// `DEVPAGE_TEST_SOFTWARE_GPU` is set: for CI runners, which have no GPU,
    /// so the GPU tests still draw on WebGPU.
    fn software_gpu(chrome: BrowserConfigBuilder) -> BrowserConfigBuilder {
        if std::env::var_os("DEVPAGE_TEST_SOFTWARE_GPU").is_none() {
            return chrome;
        }
        chrome
            .arg(("use-webgpu-adapter", "swiftshader"))
            .arg("enable-unsafe-swiftshader")
    }

    /// The run's one Chrome, as chromiumoxide drives it.
    pub(crate) async fn chrome() -> &'static chromiumoxide::Browser {
        &shared().await.chrome.browser
    }

    impl Browser {
        /// A new tab, its screen set, at `url`.
        pub(crate) async fn new_page(&self, url: impl Into<String>) -> chromiumoxide::Result<Page> {
            let mut blank = CreateTargetParams::new("about:blank");
            blank.browser_context_id = self.context.clone();
            let page = chrome().await.new_page(blank).await?;
            if let Some((width, height)) = self.screen {
                page.execute(SetDeviceMetricsOverrideParams::new(
                    i64::from(width),
                    i64::from(height),
                    1.0,
                    false,
                ))
                .await?;
            }
            let url = url.into();
            if url != "about:blank" {
                page.goto(url).await?;
            }
            Ok(page)
        }

        /// Closes every tab of the browser and forgets its storage.
        pub(crate) async fn close(&mut self) -> chromiumoxide::Result<()> {
            if let Some(context) = self.context.take() {
                chrome().await.dispose_browser_context(context).await?;
            }
            Ok(())
        }
    }

    impl Drop for Browser {
        fn drop(&mut self) {
            if let (Some(context), Some(shared)) = (self.context.take(), SHARED.get()) {
                let chrome = &shared.chrome.browser;
                shared.runtime.spawn(async move {
                    chrome.dispose_browser_context(context).await.ok();
                });
            }
        }
    }

    /// `router` served on a free loopback port for the test's lifetime: its
    /// address, `http://127.0.0.1:<port>`.
    pub(crate) async fn serve(router: Router) -> String {
        let (address, server) = super::serve(router).await.unwrap();
        // Detached: it serves until the test's runtime ends.
        drop(server);
        format!("http://{address}")
    }

    /// Every request a router answered, as `(path, status)`, in order.
    pub(crate) type Answered = Arc<Mutex<Vec<(String, u16)>>>;

    /// `router`, keeping every request it answers in the [`Answered`] it
    /// hands back.
    pub(crate) fn answering(router: Router) -> (Router, Answered) {
        let answered = Answered::default();
        let log = answered.clone();
        let router = router.layer(axum::middleware::from_fn(
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
        (router, answered)
    }

    /// Waits until `answered` holds a request for `path` beyond its first
    /// `from`, however long a loaded machine takes, for up to 20 s; panics
    /// naming it if none comes.
    pub(crate) async fn answered_after(answered: &Answered, from: usize, path: &str) {
        let started = tokio::time::Instant::now();
        while started.elapsed() < READY_WITHIN {
            if answered.lock().unwrap()[from..]
                .iter()
                .any(|(answered, _)| answered == path)
            {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("never answered: {path}");
    }

    /// `js` evaluated in `page`, its value read as a `T`.
    pub(crate) async fn eval<T: DeserializeOwned>(page: &Page, js: &str) -> T {
        page.evaluate(js).await.unwrap().into_value().unwrap()
    }

    /// Waits until `js` is truthy in `page`, however long a loaded machine
    /// takes, for up to 20 s; panics naming it if it never is. A page being
    /// loaded whole has no context to ask yet, which counts as not yet.
    pub(crate) async fn until(page: &Page, js: &str) {
        let asked = format!("!!({js})");
        let started = tokio::time::Instant::now();
        while started.elapsed() < READY_WITHIN {
            let truthy = page
                .evaluate(asked.as_str())
                .await
                .ok()
                .and_then(|answer| answer.into_value::<bool>().ok());
            if truthy == Some(true) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("never true: {js}");
    }

    /// Waits until `js`, made a string, is `expected` in `page`, for up to
    /// 20 s; panics with what it last was if it never is.
    pub(crate) async fn until_is(page: &Page, js: &str, expected: &str) {
        let asked = format!("String({js})");
        let started = tokio::time::Instant::now();
        let mut last = None;
        while started.elapsed() < READY_WITHIN {
            last = page
                .evaluate(asked.as_str())
                .await
                .ok()
                .and_then(|answer| answer.into_value::<String>().ok());
            if last.as_deref() == Some(expected) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("{js} is {last:?}, never {expected:?}");
    }

    /// Waits until the router has answered `count` requests whose path
    /// `path` accepts, for up to 20 s; panics with what it answered if it
    /// never has.
    pub(crate) async fn until_answered(
        answered: &Answered,
        path: impl Fn(&str) -> bool,
        count: usize,
    ) {
        let started = tokio::time::Instant::now();
        while started.elapsed() < READY_WITHIN {
            if answered
                .lock()
                .unwrap()
                .iter()
                .filter(|(p, _)| path(p))
                .count()
                >= count
            {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("never answered {count}: {:?}", answered.lock().unwrap());
    }

    /// Waits until `page` has drawn two more frames: whatever it painted
    /// before is on the screen.
    pub(crate) async fn next_frames(page: &Page) {
        eval::<bool>(
            page,
            "new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(() => done(true))))",
        )
        .await;
    }

    /// Waits until `page` has loaded and its script has finished starting:
    /// its widgets mounted.
    pub(crate) async fn loaded(page: &Page) {
        until(
            page,
            "document.readyState === 'complete' \
             && document.documentElement.dataset.ready === 'true'",
        )
        .await;
    }

    /// `url` opened in a new tab of `browser`, once it has [`loaded`].
    pub(crate) async fn open(browser: &Browser, url: &str) -> Page {
        let page = browser.new_page(url).await.unwrap();
        loaded(&page).await;
        page
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chromes_sandbox_is_on_unless_turned_off() {
        let launch = |setting| {
            Profile::Shot
                .config(Path::new("chrome"), Path::new("profile"), setting)
                .map(|chrome| format!("{chrome:?}"))
        };
        let off = launch(Some("off")).unwrap();
        assert!(off.contains("sandbox: false"), "{off}");
        for on in [None, Some(""), Some("on")] {
            let config = launch(on).unwrap();
            assert!(config.contains("sandbox: true"), "{on:?}: {config}");
        }
        assert!(launch(Some("no")).is_err());
        let recording = Profile::Recording
            .config(Path::new("chrome"), Path::new("profile"), Some("off"))
            .map(|chrome| format!("{chrome:?}"))
            .unwrap();
        assert!(recording.contains("sandbox: false"), "{recording}");
    }

    #[test]
    fn the_shot_profile_draws_with_the_gpu_and_the_recording_one_captures_its_tab() {
        let config = |profile: Profile| {
            format!(
                "{:?}",
                profile
                    .config(Path::new("chrome"), Path::new("profile"), None)
                    .unwrap()
            )
        };
        let shot = config(Profile::Shot);
        assert!(shot.contains("\"enable-gpu\""), "{shot}");
        assert!(shot.contains("\"enable-unsafe-webgpu\""), "{shot}");
        assert!(!shot.contains("tab-capture"), "{shot}");
        let recording = config(Profile::Recording);
        for flag in [
            "\"auto-accept-this-tab-capture\"",
            "\"no-user-gesture-required\"",
            "\"hide-scrollbars\"",
        ] {
            assert!(recording.contains(flag), "{flag}: {recording}");
        }
        assert!(!recording.contains("\"enable-gpu\""), "{recording}");
    }

    /// Where the pinned Chrome's programs sit in a cache folder, on the
    /// platforms the tests run on: macOS, and Linux and Windows on x86-64.
    #[cfg(any(
        target_os = "macos",
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86_64")
    ))]
    fn pinned_programs() -> [(BrowserKind, &'static str, PathBuf); 2] {
        let (major, minor, build, patch) = CHROME_VERSION;
        let version = format!("{major}.{minor}.{build}.{patch}");
        let platform = cft_platform();
        let folder = if cfg!(target_os = "linux") {
            "linux"
        } else if cfg!(windows) {
            "win64"
        } else if cfg!(target_arch = "aarch64") {
            "mac_arm"
        } else {
            "mac"
        };
        let folder = Path::new(&format!("{folder}-{version}")).to_path_buf();
        let chrome = if cfg!(target_os = "linux") {
            Path::new("chrome").to_path_buf()
        } else if cfg!(windows) {
            Path::new("chrome.exe").to_path_buf()
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
                    .join(if cfg!(windows) {
                        "chrome-headless-shell.exe"
                    } else {
                        "chrome-headless-shell"
                    }),
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
    #[cfg(any(
        target_os = "macos",
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86_64")
    ))]
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
    #[cfg(any(
        target_os = "macos",
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86_64")
    ))]
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
    #[cfg(any(
        target_os = "macos",
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86_64")
    ))]
    fn cft_platform() -> &'static str {
        if cfg!(target_os = "linux") {
            "linux64"
        } else if cfg!(windows) {
            "win64"
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
        let mut urls = Vec::new();
        for kind in [BrowserKind::ChromeHeadlessShell, BrowserKind::Chrome] {
            for &platform in Platform::all() {
                urls.push(kind.download_url(platform, &version, &BrowserHost::current(kind)));
            }
        }
        // Asked all at once: each answer is a round trip to Google's server.
        let asked = urls.into_iter().map(|url| async {
            let status = client.head(&url).send().await.unwrap().status();
            (url, status)
        });
        for (url, status) in futures::future::join_all(asked).await {
            assert_eq!(status, reqwest::StatusCode::OK, "{url}");
        }
    }

    /// Two tests' browsers are one Chrome — the second launches nothing —
    /// each with storage of its own. Drives chrome-headless-shell; run with
    /// `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn every_tests_browser_is_one_chrome_with_storage_of_its_own() {
        use chromiumoxide::cdp::browser_protocol::target::GetTargetsParams;
        let page = "<html><body>storage</body></html>";
        let address = testing::serve(Router::new().route(
            "/",
            axum::routing::get(move || async move { axum::response::Html(page) }),
        ))
        .await;
        let one = testing::browser().await;
        let two = testing::browser().await;
        let a = one.new_page(address.as_str()).await.unwrap();
        let b = two.new_page(address.as_str()).await.unwrap();
        let targets = testing::chrome()
            .await
            .execute(GetTargetsParams::default())
            .await
            .unwrap();
        for page in [&a, &b] {
            assert!(
                targets
                    .result
                    .target_infos
                    .iter()
                    .any(|t| &t.target_id == page.target_id()),
                "a browser's page is in another Chrome"
            );
        }
        testing::eval::<bool>(&a, "(localStorage.setItem('kept', 'one'), true)").await;
        assert_eq!(
            testing::eval::<String>(&b, "String(localStorage.getItem('kept'))").await,
            "null",
            "the second browser sees the first's storage"
        );
    }

    /// A session's tab opens its page once it is ready, carries the shot
    /// header only when opened to be shot, and is evaluated in.
    /// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn a_tab_sends_the_shot_header_only_when_opened_to_be_shot() {
        use axum::http::HeaderMap;
        use axum::routing::get;
        let page = "<html data-ready=\"true\"><script>\
            document.documentElement.dataset.ready = 'true';\
            fetch('/shot').then((r) => r.text()).then((t) => { document.title = t; });\
            </script></html>";
        let router =
            Router::new()
                .route("/", get(move || async move { axum::response::Html(page) }))
                .route(
                    "/shot",
                    get(|headers: HeaderMap| async move {
                        headers.contains_key(SHOT_HEADER).to_string()
                    }),
                );
        let session = Session::start(Profile::Shot, router).await.unwrap();
        assert!(session.running());
        for shot in [false, true] {
            let tab = session
                .open(
                    "/",
                    Open {
                        shot,
                        ..Open::default()
                    },
                )
                .await
                .unwrap();
            assert!(tab.ready());
            let heard = format!("document.title === '{shot}'");
            assert!(tab.until(&heard, Duration::from_secs(5)).await.unwrap());
            assert_eq!(tab.eval::<i64>("Promise.resolve(6 * 7)").await.unwrap(), 42);
            tab.close().await;
        }
        session.close().await;
    }
}
