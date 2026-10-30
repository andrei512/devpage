//! The server's metrics: how long each stage of its work took, as
//! histograms, each request by its route and status, and what the server
//! holds, served in the Prometheus text format on the metrics port. Every
//! label is a stage from the closed set below, a route as the router declares
//! it, or a status — never a document, a version or a path asked for.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{MatchedPath, Request};
use axum::middleware::Next;
use axum::response::Response;
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Event, Metadata, Subscriber};

/// The stages a request can spend its time in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Waiting for the store's lock, and the query.
    Store,
    /// A version's zip from the bucket, on a cache miss.
    Fetch,
    /// The zip into the cache.
    Unpack,
    /// A version's router made: its document loaded — its search index waits
    /// for its first search.
    Router,
    /// The page's problems: its links, and the pages they reach.
    Check,
    /// The site's navigation, around the page.
    Nav,
    /// The page in its frame — its navigation among it.
    Render,
    /// A publish checked before it is accepted.
    PublishCheck,
    /// A publish's zip into the bucket.
    Upload,
    /// A version's PDF or PNG drawn, on its first request.
    Download,
}

const STAGES: [Stage; 10] = [
    Stage::Store,
    Stage::Fetch,
    Stage::Unpack,
    Stage::Router,
    Stage::Check,
    Stage::Nav,
    Stage::Render,
    Stage::PublishCheck,
    Stage::Upload,
    Stage::Download,
];

impl Stage {
    fn name(self) -> &'static str {
        match self {
            Stage::Store => "store",
            Stage::Fetch => "fetch",
            Stage::Unpack => "unpack",
            Stage::Router => "router",
            Stage::Check => "check",
            Stage::Nav => "nav",
            Stage::Render => "render",
            Stage::PublishCheck => "publish-check",
            Stage::Upload => "upload",
            Stage::Download => "download",
        }
    }

    /// The stage a core span of this name times, if it times one: the core
    /// times `check`, `nav` and `render`, and server mode the rest.
    fn of_span(name: &str) -> Option<Stage> {
        [Stage::Check, Stage::Nav, Stage::Render]
            .into_iter()
            .find(|stage| stage.name() == name)
    }
}

/// Upper bounds of the buckets, in seconds.
const BOUNDS: [f64; 14] = [
    0.0005, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

#[derive(Default)]
struct Histogram {
    buckets: [AtomicU64; BOUNDS.len()],
    count: AtomicU64,
    micros: AtomicU64,
}

impl Histogram {
    fn observe(&self, took: Duration) {
        let seconds = took.as_secs_f64();
        if let Some(at) = BOUNDS.iter().position(|bound| seconds <= *bound) {
            self.buckets[at].fetch_add(1, Ordering::Relaxed);
        }
        self.count.fetch_add(1, Ordering::Relaxed);
        self.micros.fetch_add(
            u64::try_from(took.as_micros()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
    }
}

static HISTOGRAMS: LazyLock<[Histogram; STAGES.len()]> = LazyLock::new(Default::default);

fn histogram(stage: Stage) -> &'static Histogram {
    let at = STAGES.iter().position(|s| *s == stage).unwrap_or(0);
    &HISTOGRAMS[at]
}

/// Counts `took` against `stage`.
pub fn observe(stage: Stage, took: Duration) {
    histogram(stage).observe(took);
}

/// Times `work` as `stage`.
pub fn time<T>(stage: Stage, work: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let done = work();
    observe(stage, started.elapsed());
    done
}

/// Times `work` as `stage`, from its first poll to its answer: a future
/// never awaited is never counted.
pub async fn timed<T>(stage: Stage, work: impl Future<Output = T>) -> T {
    let started = Instant::now();
    let done = work.await;
    observe(stage, started.elapsed());
    done
}

/// What the cache did, counted since the server started.
#[derive(Clone, Copy)]
pub enum Cache {
    Hit,
    Miss,
    Eviction,
}

static COUNTS: [AtomicU64; 3] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];

/// Counts one `count`.
pub fn count(count: Cache) {
    COUNTS[count as usize].fetch_add(1, Ordering::Relaxed);
}

/// Each request's time, by the route the router matched — as it declares
/// it, never the path asked for — and the status it answered.
static REQUESTS: LazyLock<Mutex<BTreeMap<(String, u16), Histogram>>> =
    LazyLock::new(Mutex::default);

/// The middleware that times every request by its route and status.
pub async fn requests(request: Request, next: Next) -> Response {
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or("unmatched", MatchedPath::as_str)
        .to_string();
    let started = Instant::now();
    let response = next.run(request).await;
    REQUESTS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .entry((route, response.status().as_u16()))
        .or_default()
        .observe(started.elapsed());
    response
}

/// What the server holds now, asked as `/metrics` is.
pub struct Totals {
    /// Versions with a router in memory.
    pub versions: usize,
    /// Their routers: a version has one per mount it was read under.
    pub routers: usize,
    /// Pages listening for their document's next version.
    pub open_pages: u64,
    /// The cache's bytes on the PVC.
    pub cache_bytes: u64,
}

/// Every histogram, count and total, in the Prometheus text format.
pub fn text(totals: &Totals) -> String {
    let mut out = String::from(
        "# HELP devpage_stage_seconds Time spent in each stage of the server's work.\n\
         # TYPE devpage_stage_seconds histogram\n",
    );
    for stage in STAGES {
        let labels = format!("stage=\"{}\"", stage.name());
        write_histogram(&mut out, "devpage_stage_seconds", &labels, histogram(stage));
    }
    out.push_str(
        "# HELP devpage_request_seconds Each request's time, by route and status.\n\
         # TYPE devpage_request_seconds histogram\n",
    );
    for ((route, status), histogram) in REQUESTS.lock().unwrap_or_else(|p| p.into_inner()).iter() {
        let labels = format!("route=\"{route}\",status=\"{status}\"");
        write_histogram(&mut out, "devpage_request_seconds", &labels, histogram);
    }
    let (threads, resident) = process();
    let counts = |count: Cache| COUNTS[count as usize].load(Ordering::Relaxed);
    for (name, kind, help, value) in [
        (
            "devpage_versions_in_memory",
            "gauge",
            "Versions with a router in memory.",
            totals.versions as u64,
        ),
        (
            "devpage_routers",
            "gauge",
            "Version routers in memory, one per mount read.",
            totals.routers as u64,
        ),
        ("devpage_threads", "gauge", "The server's threads.", threads),
        (
            "devpage_open_pages",
            "gauge",
            "Pages listening for their document's next version.",
            totals.open_pages,
        ),
        (
            "devpage_cache_bytes",
            "gauge",
            "The cache's bytes on the PVC.",
            totals.cache_bytes,
        ),
        (
            "devpage_cache_hits_total",
            "counter",
            "Versions and downloads read from the cache.",
            counts(Cache::Hit),
        ),
        (
            "devpage_cache_misses_total",
            "counter",
            "Versions and downloads not in the cache.",
            counts(Cache::Miss),
        ),
        (
            "devpage_cache_evictions_total",
            "counter",
            "Versions evicted from the cache.",
            counts(Cache::Eviction),
        ),
        (
            "devpage_resident_bytes",
            "gauge",
            "The server's resident memory.",
            resident,
        ),
    ] {
        let _ = writeln!(
            out,
            "# HELP {name} {help}\n# TYPE {name} {kind}\n{name} {value}"
        );
    }
    out
}

fn write_histogram(out: &mut String, name: &str, labels: &str, histogram: &Histogram) {
    let mut cumulative = 0;
    for (bound, bucket) in BOUNDS.iter().zip(&histogram.buckets) {
        cumulative += bucket.load(Ordering::Relaxed);
        let _ = writeln!(out, "{name}_bucket{{{labels},le=\"{bound}\"}} {cumulative}");
    }
    let count = histogram.count.load(Ordering::Relaxed);
    let seconds = histogram.micros.load(Ordering::Relaxed) as f64 / 1e6;
    let _ = writeln!(out, "{name}_bucket{{{labels},le=\"+Inf\"}} {count}");
    let _ = writeln!(out, "{name}_sum{{{labels}}} {seconds}");
    let _ = writeln!(out, "{name}_count{{{labels}}} {count}");
}

/// The process's threads and resident bytes, as the kernel counts them.
#[cfg(target_os = "linux")]
fn process() -> (u64, u64) {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |name: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|rest| rest.split_whitespace().next()?.parse::<u64>().ok())
            .unwrap_or(0)
    };
    (field("Threads:"), field("VmRSS:") * 1024)
}

/// The process's threads and resident bytes, as the kernel counts them.
#[cfg(target_os = "macos")]
fn process() -> (u64, u64) {
    let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    // SAFETY: `info` is a proc_taskinfo of `size` bytes for the kernel to fill.
    let filled = unsafe {
        libc::proc_pidinfo(
            std::process::id() as libc::c_int,
            libc::PROC_PIDTASKINFO,
            0,
            (&raw mut info).cast(),
            size,
        )
    };
    if filled != size {
        return (0, 0);
    }
    (info.pti_threadnum as u64, info.pti_resident_size)
}

/// The process's threads and resident bytes, as the kernel counts them: the
/// threads in a snapshot of the system's, and the working set.
#[cfg(windows)]
fn process() -> (u64, u64) {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let id = std::process::id();
    let mut threads = 0;
    // SAFETY: the snapshot handle is checked, walked with entries of the
    // size they say, and closed.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot != INVALID_HANDLE_VALUE {
            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            let mut more = Thread32First(snapshot, &mut entry) != 0;
            while more {
                if entry.th32OwnerProcessID == id {
                    threads += 1;
                }
                more = Thread32Next(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
        }
    }
    let mut memory: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    // SAFETY: `memory` is a PROCESS_MEMORY_COUNTERS of `size` bytes to fill,
    // for this process's pseudo-handle.
    let read = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut memory, size) } != 0;
    (
        threads,
        if read {
            memory.WorkingSetSize as u64
        } else {
            0
        },
    )
}

/// Elsewhere the kernel isn't asked: both read 0.
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn process() -> (u64, u64) {
    (0, 0)
}

/// The listener that turns the core's stage spans into histograms: a span is
/// timed from when it is made to when it closes. Every other span and every
/// event is ignored.
#[derive(Default)]
pub struct Stages {
    next: AtomicU64,
    open: Mutex<HashMap<u64, (Stage, Instant)>>,
}

impl Subscriber for Stages {
    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        if self.enabled(metadata) {
            Interest::always()
        } else {
            Interest::never()
        }
    }

    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.is_span() && Stage::of_span(metadata.name()).is_some()
    }

    fn new_span(&self, span: &Attributes<'_>) -> Id {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        if let Some(stage) = Stage::of_span(span.metadata().name()) {
            self.open
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(id, (stage, Instant::now()));
        }
        Id::from_u64(id)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, _: &Event<'_>) {}

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}

    fn try_close(&self, id: Id) -> bool {
        let closed = self
            .open
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&id.into_u64());
        if let Some((stage, started)) = closed {
            observe(stage, started.elapsed());
        }
        true
    }
}

/// Makes [`Stages`] the process's listener, once: from then on the core's
/// stage spans are timed, on every thread.
pub fn listen() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = tracing::subscriber::set_global_default(Stages::default());
    });
}

/// The metrics port's router: `/metrics`, and nothing else; `totals` says
/// what the server holds as each scrape asks.
pub fn router(totals: impl Fn() -> Totals + Clone + Send + Sync + 'static) -> axum::Router {
    axum::Router::new().route(
        "/metrics",
        axum::routing::get(move || async move {
            (
                [(
                    axum::http::header::CONTENT_TYPE,
                    "text/plain; version=0.0.4",
                )],
                text(&totals()),
            )
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_core_span_is_counted_against_its_stage() {
        let before = histogram(Stage::Nav).count.load(Ordering::Relaxed);
        tracing::subscriber::with_default(Stages::default(), || {
            let _nav = tracing::info_span!("nav").entered();
            let _other = tracing::info_span!("anything else").entered();
        });
        assert_eq!(
            histogram(Stage::Nav).count.load(Ordering::Relaxed),
            before + 1
        );
    }

    #[test]
    fn the_text_names_only_stages() {
        observe(Stage::Download, Duration::from_millis(3));
        let text = text(&Totals {
            versions: 0,
            routers: 0,
            open_pages: 0,
            cache_bytes: 0,
        });
        assert!(text.contains("devpage_stage_seconds_bucket{stage=\"download\",le=\"0.005\"}"));
        for line in text
            .lines()
            .filter(|l| l.starts_with("devpage_stage_seconds"))
        {
            let labels = &line[line.find('{').unwrap()..line.find('}').unwrap()];
            let stage = labels.split('"').nth(1).unwrap();
            assert!(STAGES.iter().any(|s| s.name() == stage), "{line}");
        }
    }
}
